use crate::CorpusId;
use serde::Serialize;
use std::{
	error::Error,
	fmt,
	fs::File,
	io::{self, Read},
	path::{Component, Path, PathBuf},
};

const MAX_APPROVED_SOURCES: usize = 32;
const MAX_SOURCE_BYTES: usize = 64 * 1024;
const MAX_TAG_BYTES: usize = 128;
const MAX_RELATIVE_PATH_BYTES: usize = 512;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Compatibility {
	pub engine_version: String,
	pub tool_schema_version: u32,
}

impl Compatibility {
	pub fn new(engine_version: impl Into<String>, tool_schema_version: u32) -> Self {
		Self {
			engine_version: engine_version.into(),
			tool_schema_version,
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchScope {
	pub compatibility: Compatibility,
	pub project_id: Option<String>,
}

impl SearchScope {
	pub fn new(compatibility: Compatibility, project_id: Option<String>) -> Self {
		Self {
			compatibility,
			project_id,
		}
	}
}

/// A single approved relative file. The corpus root comes from the host.
#[derive(Debug)]
pub struct ApprovedSource {
	pub(super) id: String,
	pub(super) corpus: CorpusId,
	path: PathBuf,
	pub(super) display_path: String,
}

impl ApprovedSource {
	pub fn new(
		corpus: CorpusId,
		id: impl Into<String>,
		path: impl Into<PathBuf>,
	) -> Result<Self, CorpusError> {
		let id = id.into();
		if !valid_tag(&id) {
			return Err(CorpusError::InvalidSourceId(id));
		}
		let path = path.into();
		let display_path = approved_path(&path)?;
		Ok(Self {
			id,
			corpus,
			path,
			display_path,
		})
	}

	pub(super) fn is_markdown(&self) -> bool {
		self.path
			.extension()
			.is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
	}
}

/// The trusted host supplies explicit source paths. There is no recursive discovery.
pub struct CorpusManifest {
	scope: SearchScope,
	engine_root: PathBuf,
	project_root: Option<PathBuf>,
	pub(super) sources: Vec<ApprovedSource>,
}

impl CorpusManifest {
	pub fn new(
		compatibility: Compatibility,
		engine_root: &Path,
		project: Option<(&str, &Path)>,
		sources: Vec<ApprovedSource>,
	) -> Result<Self, CorpusError> {
		if sources.len() > MAX_APPROVED_SOURCES {
			return Err(CorpusError::Budget("approved sources"));
		}
		if !valid_tag(&compatibility.engine_version)
			|| project.is_some_and(|(id, _)| !valid_tag(id))
		{
			return Err(CorpusError::InvalidScope);
		}
		for (index, source) in sources.iter().enumerate() {
			if source.corpus == CorpusId::ActiveProject && project.is_none() {
				return Err(CorpusError::InvalidScope);
			}
			if sources[..index].iter().any(|existing| {
				existing.corpus == source.corpus
					&& (existing.id == source.id || existing.path == source.path)
			}) {
				return Err(CorpusError::DuplicateSource(source.id.clone()));
			}
		}
		Ok(Self {
			scope: SearchScope::new(compatibility, project.map(|(id, _)| id.to_owned())),
			engine_root: canonical_root(engine_root)?,
			project_root: project.map(|(_, root)| canonical_root(root)).transpose()?,
			sources,
		})
	}

	pub fn scope(&self) -> &SearchScope {
		&self.scope
	}

	pub(super) fn read(&self, source: &ApprovedSource) -> Result<Option<String>, CorpusError> {
		let root = match source.corpus {
			CorpusId::EngineHelp => &self.engine_root,
			CorpusId::ActiveProject => self
				.project_root
				.as_ref()
				.ok_or(CorpusError::InvalidScope)?,
		};
		let path = root.join(&source.path);
		let resolved = match path.canonicalize() {
			Ok(resolved) => resolved,
			Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
			Err(error) => return Err(source_error(&source.path, error)),
		};
		let relative = resolved
			.strip_prefix(root)
			.map_err(|_| CorpusError::UnapprovedPath(source.path.clone()))?;
		approved_path(relative)?;

		let file = File::open(&resolved).map_err(|error| source_error(&source.path, error))?;
		let metadata = file
			.metadata()
			.map_err(|error| source_error(&source.path, error))?;
		if !metadata.is_file() {
			return Err(CorpusError::UnapprovedPath(source.path.clone()));
		}
		if metadata.len() > MAX_SOURCE_BYTES as u64 {
			return Err(CorpusError::Budget("source bytes"));
		}
		let mut bytes = Vec::new();
		file.take((MAX_SOURCE_BYTES + 1) as u64)
			.read_to_end(&mut bytes)
			.map_err(|error| source_error(&source.path, error))?;
		if bytes.len() > MAX_SOURCE_BYTES {
			return Err(CorpusError::Budget("source bytes"));
		}
		String::from_utf8(bytes)
			.map(Some)
			.map_err(|error| source_error(&source.path, error))
	}
}

#[derive(Debug)]
pub enum CorpusError {
	InvalidScope,
	InvalidSourceId(String),
	DuplicateSource(String),
	UnapprovedPath(PathBuf),
	Budget(&'static str),
	Source { path: PathBuf, message: String },
}

impl fmt::Display for CorpusError {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::InvalidScope => {
				formatter.write_str("Knowledge corpus has an invalid project or version scope")
			}
			Self::InvalidSourceId(id) => {
				write!(formatter, "Invalid knowledge source identifier {id}")
			}
			Self::DuplicateSource(id) => {
				write!(formatter, "Duplicate approved knowledge source {id}")
			}
			Self::UnapprovedPath(path) => write!(
				formatter,
				"Knowledge source path is outside the approved text corpus: {}",
				path.display()
			),
			Self::Budget(resource) => {
				write!(formatter, "Knowledge corpus exceeds its {resource} budget")
			}
			Self::Source { path, message } => write!(
				formatter,
				"Cannot read knowledge source {}: {message}",
				path.display()
			),
		}
	}
}

impl Error for CorpusError {}

fn source_error(path: &Path, error: impl fmt::Display) -> CorpusError {
	CorpusError::Source {
		path: path.to_owned(),
		message: error.to_string(),
	}
}

fn canonical_root(path: &Path) -> Result<PathBuf, CorpusError> {
	let resolved = path
		.canonicalize()
		.map_err(|error| source_error(path, error))?;
	if !resolved.is_dir() {
		return Err(CorpusError::UnapprovedPath(path.to_owned()));
	}
	Ok(resolved)
}

fn valid_tag(tag: &str) -> bool {
	!tag.is_empty()
		&& tag.len() <= MAX_TAG_BYTES
		&& tag
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}

fn approved_path(path: &Path) -> Result<String, CorpusError> {
	let reject = || CorpusError::UnapprovedPath(path.to_owned());
	let mut segments = Vec::new();
	for component in path.components() {
		let Component::Normal(segment) = component else {
			return Err(reject());
		};
		let segment = segment.to_str().ok_or_else(reject)?;
		if forbidden_segment(segment) {
			return Err(reject());
		}
		segments.push(segment);
	}
	let display = segments.join("/");
	if display.len() > MAX_RELATIVE_PATH_BYTES
		|| !path.extension().is_some_and(|extension| {
			["md", "json", "txt"]
				.iter()
				.any(|kind| extension.eq_ignore_ascii_case(kind))
		}) {
		return Err(reject());
	}
	Ok(display)
}

fn forbidden_segment(segment: &str) -> bool {
	let name = segment.to_ascii_lowercase();
	let stem = name.split('.').next().unwrap_or_default();
	name.starts_with('.')
		|| name.contains(':')
		|| name.ends_with([' ', '.'])
		|| name.chars().any(char::is_control)
		|| matches!(
			stem,
			"target"
				| "node_modules"
				| "dist" | "build"
				| "generated"
				| "logs" | "credentials"
				| "secrets" | "secret"
				| "token" | "tokens"
				| "private-key"
				| "id_rsa" | "id_ed25519"
		)
}
