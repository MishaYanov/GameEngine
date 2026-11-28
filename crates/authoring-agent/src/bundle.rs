use crate::tools::{RegisteredTool, ToolDescriptor, ToolError, ToolId, ToolScope, ToolService};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{error::Error, fmt};

const BUNDLE_ID: &str = "game-development";
const BUNDLE_VERSION: u32 = 1;
const EDITOR_PROFILE: &str = "editor-guidance";
const MAX_MANIFEST_BYTES: usize = 8 * 1024;
const MANIFEST: &[u8] = include_bytes!(concat!(
	env!("CARGO_MANIFEST_DIR"),
"/profiles/game-development-support.json"
));
const WORKFLOW_GUIDANCE: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../.agents/skills/game-development-support/SKILL.md"
));

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CorpusId {
	EngineHelp,
	ActiveProject,
}

/// A validated set of read-only tools and retrieval sources.
#[derive(Debug)]
pub struct Bundle {
	tools: Vec<ToolId>,
	corpora: Vec<CorpusId>,
}

impl Bundle {
	pub fn builtin() -> Result<Self, BundleError> {
		Self::from_json(MANIFEST)
	}

	pub fn from_json(bytes: &[u8]) -> Result<Self, BundleError> {
		if bytes.len() > MAX_MANIFEST_BYTES {
			return Err(BundleError::TooLarge);
		}
		let manifest: Manifest = serde_json::from_slice(bytes).map_err(BundleError::InvalidJson)?;
		if manifest.version != BUNDLE_VERSION {
			return Err(BundleError::UnsupportedVersion(manifest.version));
		}
		if manifest.id != BUNDLE_ID {
			return Err(BundleError::UnknownBundle(manifest.id));
		}
		if manifest.profile != EDITOR_PROFILE {
			return Err(BundleError::UnknownProfile(manifest.profile));
		}
		let mut tools = Vec::new();
		for name in manifest.tools {
			let tool = RegisteredTool::find(&name)
				.ok_or_else(|| BundleError::ToolOutsideProfile(name.clone()))?;
			let id = tool.id();
			if tools.contains(&id) {
				return Err(BundleError::DuplicateTool(name));
			}
			tools.push(id);
		}
		for required in [
			ToolId::EditorInspect,
			ToolId::AssetsQuery,
			ToolId::KnowledgeSearch,
		] {
			if !tools.contains(&required) {
				return Err(BundleError::MissingTool(required.name()));
			}
		}
		let mut corpora = Vec::new();
		for corpus in manifest.corpora {
			if corpora.contains(&corpus) {
				return Err(BundleError::DuplicateCorpus(corpus));
			}
			corpora.push(corpus);
		}
		for required in [CorpusId::EngineHelp, CorpusId::ActiveProject] {
			if !corpora.contains(&required) {
				return Err(BundleError::MissingCorpus(required));
			}
		}
		Ok(Self { tools, corpora })
	}

	pub fn instructions(&self) -> &'static str {
		WORKFLOW_GUIDANCE
	}

	pub fn corpora(&self) -> &[CorpusId] {
		&self.corpora
	}

	/// Discover only tools supported by a live adapter in this captured scope.
	pub fn available_tools(
		&self,
		service: &dyn ToolService,
		scope: &ToolScope,
	) -> Vec<ToolDescriptor> {
		self.tools
			.iter()
			.copied()
			.filter(|id| service.supports(scope, *id))
			.map(|id| RegisteredTool::by_id(id).descriptor())
			.collect()
	}

	/// Execute a typed read using host-supplied project and revision identity.
	pub fn invoke(
		&self,
		service: &dyn ToolService,
		scope: &ToolScope,
		name: &str,
		arguments_json: &[u8],
	) -> Result<Value, ToolError> {
		let tool = RegisteredTool::find(name).ok_or(ToolError::Denied)?;
		let id = tool.id();
		if !self.tools.contains(&id) {
			return Err(ToolError::Denied);
		}
		if !service.supports(scope, id) {
			return Err(ToolError::Unavailable(id));
		}
		tool.invoke(service, scope, arguments_json)
	}
}

#[derive(Debug)]
pub enum BundleError {
	TooLarge,
	InvalidJson(serde_json::Error),
	UnsupportedVersion(u32),
	UnknownBundle(String),
	UnknownProfile(String),
	ToolOutsideProfile(String),
	DuplicateTool(String),
	MissingTool(&'static str),
	DuplicateCorpus(CorpusId),
	MissingCorpus(CorpusId),
}

impl fmt::Display for BundleError {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::TooLarge => formatter.write_str("Agent manifest exceeds its size limit"),
			Self::InvalidJson(error) => write!(formatter, "Invalid agent manifest: {error}"),
			Self::UnsupportedVersion(version) => {
				write!(formatter, "Unsupported agent bundle version {version}")
			}
			Self::UnknownBundle(id) => write!(formatter, "Unknown agent bundle {id}"),
			Self::UnknownProfile(profile) => write!(formatter, "Unknown agent profile {profile}"),
			Self::ToolOutsideProfile(tool) => write!(
				formatter,
				"{tool} is outside the editor-guidance read-only tool grant"
			),
			Self::DuplicateTool(tool) => write!(formatter, "Duplicate agent tool {tool}"),
			Self::MissingTool(tool) => write!(formatter, "Agent bundle is missing {tool}"),
			Self::DuplicateCorpus(corpus) => write!(formatter, "Duplicate agent corpus {corpus:?}"),
			Self::MissingCorpus(corpus) => {
				write!(formatter, "Agent bundle is missing corpus {corpus:?}")
			}
		}
	}
}

impl Error for BundleError {
	fn source(&self) -> Option<&(dyn Error + 'static)> {
		match self {
			Self::InvalidJson(error) => Some(error),
			_ => None,
		}
	}
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
	id: String,
	version: u32,
	profile: String,
	tools: Vec<String>,
	corpora: Vec<CorpusId>,
}
