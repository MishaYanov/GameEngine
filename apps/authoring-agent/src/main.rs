//! Stdio transport for one explicitly scoped local authoring worker.

mod mcp;
mod ollama;

use authoring_agent::{
	Bundle, CorpusId,
	knowledge::{
		ApprovedSource, Compatibility, CorpusManifest, KnowledgeIndex, ReferenceHit, lexical_search,
	},
	session::{
		CancellationToken, MAX_CITATIONS, MAX_WORKER_LINE_BYTES, ReferenceProvider, Session,
		WORKER_PROTOCOL_VERSION, WorkerEvent, WorkerInput, WorkerScope, read_frame,
	},
};
use std::{
	env,
	ffi::{OsStr, OsString},
	fs::File,
	io::{self, Read, Write},
	path::{Path, PathBuf},
};

const ENGINE_HELP: &[(&str, &str)] = &[
	("editor-workspace", "editor-workspace.md"),
	("object-components", "authoring-objects.md"),
	("authoring-play", "scene-editing.md"),
	("game-workflows", "project-lifecycle.md"),
];
const TOOL_SCHEMA_VERSION: u32 = 1;
const MAX_BUNDLE_BYTES: u64 = 8 * 1024;

struct Options {
	resource_root: PathBuf,
	project_root: Option<PathBuf>,
	scope: WorkerScope,
}

impl Options {
	fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, &'static str> {
		let mut args = args.into_iter();
		let mut resources = None;
		let mut project = None;
		let mut project_id = None;
		let mut generation = None;
		let mut protocol_version = None;
		while let Some(flag) = args.next() {
			let value = args.next().ok_or("Agent worker option needs a value")?;
			let target = match flag.to_str() {
				Some("--resources") => &mut resources,
				Some("--project-root") => &mut project,
				Some("--project-id") => &mut project_id,
				Some("--workspace-generation") => &mut generation,
				Some("--protocol-version") => &mut protocol_version,
				_ => return Err("Unknown Agent worker option"),
			};
			if target.replace(value).is_some() {
				return Err("Agent worker option appears twice");
			}
		}

		if protocol_version.as_deref().is_some_and(|value| {
			value.to_str().and_then(|version| version.parse().ok()) != Some(WORKER_PROTOCOL_VERSION)
		}) {
			return Err("Agent worker protocol version differs from the Editor");
		}
		let resource_root = resources
			.map(PathBuf::from)
			.ok_or("Agent resources are required")?;
		let project_root = project.map(PathBuf::from);
		if !resource_root.is_absolute() || project_root.as_ref().is_some_and(|p| !p.is_absolute()) {
			return Err("Agent resource and project paths must be absolute");
		}

		let project_id = project_id
			.and_then(|id| id.into_string().ok())
			.ok_or("Agent worker requires a stable project ID")?;
		let generation = generation
			.as_deref()
			.and_then(OsStr::to_str)
			.and_then(|generation| generation.parse().ok())
			.ok_or("Agent worker requires a workspace generation")?;
		let scope =
			WorkerScope::new(project_id, generation).ok_or("Invalid Agent project scope")?;
		Ok(Self {
			resource_root,
			project_root,
			scope,
		})
	}
}

struct References {
	engine_manifest: CorpusManifest,
	project_manifest: Option<CorpusManifest>,
	engine_index: KnowledgeIndex,
	project_index: KnowledgeIndex,
}

impl References {
	fn open(engine_help: &Path, options: &Options) -> Result<Self, &'static str> {
		let compatibility = Compatibility::new(env!("CARGO_PKG_VERSION"), TOOL_SCHEMA_VERSION);
		let engine_manifest =
			CorpusManifest::new(compatibility.clone(), engine_help, None, engine_sources()?)
				.map_err(|_| "The bundled Agent help folder is unavailable")?;
		let project_manifest = options.project_root.as_deref().and_then(|root| {
			let mut sources = engine_sources().ok()?;
			sources.push(
				ApprovedSource::new(CorpusId::ActiveProject, "project", "project.json").ok()?,
			);
			CorpusManifest::new(
				compatibility,
				engine_help,
				Some((options.scope.project_id(), root)),
				sources,
			)
			.ok()
		});
		let mut engine_index = KnowledgeIndex::default();
		engine_index
			.refresh(&engine_manifest)
			.map_err(|_| "The bundled Agent help could not be indexed")?;
		Ok(Self {
			engine_manifest,
			project_manifest,
			engine_index,
			project_index: KnowledgeIndex::default(),
		})
	}
}

impl ReferenceProvider for References {
	fn search(&mut self, question: &str, cancel: &CancellationToken) -> Vec<ReferenceHit> {
		if cancel.is_cancelled() {
			return Vec::new();
		}
		if let Some(manifest) = &self.project_manifest
			&& self.project_index.refresh(manifest).is_ok()
		{
			return lexical_search(
				&self.project_index,
				manifest.scope(),
				question,
				MAX_CITATIONS,
			);
		}
		lexical_search(
			&self.engine_index,
			self.engine_manifest.scope(),
			question,
			MAX_CITATIONS,
		)
	}
}

fn engine_sources() -> Result<Vec<ApprovedSource>, &'static str> {
	ENGINE_HELP
		.iter()
		.map(|&(id, file)| ApprovedSource::new(CorpusId::EngineHelp, id, file))
		.collect::<Result<Vec<_>, _>>()
		.map_err(|_| "The Agent help allowlist is invalid")
}

fn main() {
	let mut args = env::args_os().skip(1).peekable();
	let outcome = if args.peek().is_some_and(|arg| arg == "--mcp") {
		args.next();
		if args.next().is_some() {
			Err("MCP mode uses the connection configuration in the process environment")
		} else {
			mcp::run()
		}
	} else {
		Options::parse(args).and_then(run)
	};
	if let Err(message) = outcome {
		eprintln!("{message}");
		std::process::exit(1);
	}
}

fn run(options: Options) -> Result<(), &'static str> {
	let resources = options
		.resource_root
		.canonicalize()
		.map_err(|_| "Agent resources are unavailable")?;
	let manifest = resource(&resources, "bundle.json")?;
	let mut bytes = Vec::new();
	File::open(manifest)
		.and_then(|file| file.take(MAX_BUNDLE_BYTES + 1).read_to_end(&mut bytes))
		.map_err(|_| "The packaged Agent profile is unavailable")?;
	let bundle = Bundle::from_json(&bytes).map_err(|_| "The packaged Agent profile is invalid")?;
	let engine_help = resource(&resources, "engine-help")?;
	let references = References::open(&engine_help, &options)?;
	let model = ollama::OllamaModel::from_env(bundle.instructions());
	let session = Session::new(options.scope, references, model, write_event)
		.map_err(|_| "The Agent answer thread could not start")?;
	let mut input = io::stdin().lock();
	while let Some(line) = read_frame(&mut input, MAX_WORKER_LINE_BYTES)
		.map_err(|_| "Invalid or oversized Agent input frame")?
	{
		match serde_json::from_slice::<WorkerInput>(&line) {
			Ok(input) => session.handle(input),
			Err(_) => eprintln!("Invalid Agent protocol input"),
		}
	}
	Ok(())
}

fn resource(root: &Path, name: &str) -> Result<PathBuf, &'static str> {
	let path = root
		.join(name)
		.canonicalize()
		.map_err(|_| "A packaged Agent resource is missing")?;
	if !path.starts_with(root) {
		return Err("A packaged Agent resource leaves its approved folder");
	}
	Ok(path)
}

fn write_event(event: WorkerEvent) {
	let Ok(mut bytes) = serde_json::to_vec(&event) else {
		return;
	};
	if bytes.len() >= MAX_WORKER_LINE_BYTES {
		return;
	}
	bytes.push(b'\n');
	let mut stdout = io::stdout().lock();
	let _ = stdout.write_all(&bytes).and_then(|_| stdout.flush());
}
