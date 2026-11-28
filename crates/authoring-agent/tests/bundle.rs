use authoring_agent::{
	Bundle, BundleError, CorpusId, SearchArgs, ToolError, ToolId, ToolScope, ToolService,
};
use serde_json::{Value, json};
use std::cell::Cell;

struct LiveAdapter {
	scope: ToolScope,
	calls: Cell<usize>,
	oversized: bool,
}

impl LiveAdapter {
	fn new() -> Self {
		Self {
			scope: ToolScope::new("opened-project".into(), 24, 9),
			calls: Cell::new(0),
			oversized: false,
		}
	}
}

impl ToolService for LiveAdapter {
	fn supports(&self, scope: &ToolScope, tool: ToolId) -> bool {
		scope == &self.scope && tool != ToolId::KnowledgeSearch
	}

	fn inspect(&self, scope: &ToolScope) -> Result<Value, ToolError> {
		if scope != &self.scope {
			return Err(ToolError::StaleScope);
		}
		self.calls.set(self.calls.get() + 1);
		Ok(json!({
			"project": scope.project_id(),
			"generation": scope.workspace_generation(),
			"revision": scope.revision(),
			"object": if self.oversized { "x".repeat(65_536) } else { "selected-prop".into() }
		}))
	}

	fn query_assets(&self, scope: &ToolScope, args: &SearchArgs) -> Result<Value, ToolError> {
		if scope != &self.scope {
			return Err(ToolError::StaleScope);
		}
		self.calls.set(self.calls.get() + 1);
		Ok(json!({"query": args.query(), "limit": args.limit()}))
	}

	fn search_knowledge(&self, _scope: &ToolScope, _args: &SearchArgs) -> Result<Value, ToolError> {
		panic!("the unavailable retrieval adapter must never be invoked")
	}
}

fn manifest() -> Value {
	serde_json::from_str(include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
	"/profiles/game-development-support.json"
	)))
	.unwrap()
}

fn load(manifest: &Value) -> Result<Bundle, BundleError> {
	Bundle::from_json(&serde_json::to_vec(manifest).unwrap())
}

#[test]
fn bundle_rejects_incompatible_and_ungranted_manifests() {
	let mut unknown_version = manifest();
	unknown_version["version"] = 2.into();
	assert!(matches!(
		load(&unknown_version),
		Err(BundleError::UnsupportedVersion(2))
	));

	let mut write_tool = manifest();
	write_tool["tools"]
		.as_array_mut()
		.unwrap()
		.push(json!("scene.apply_batch"));
	assert!(
		matches!(load(&write_tool), Err(BundleError::ToolOutsideProfile(id)) if id == "scene.apply_batch")
	);

	let mut missing = manifest();
	missing["tools"].as_array_mut().unwrap().pop();
	assert!(matches!(
		load(&missing),
		Err(BundleError::MissingTool("knowledge.search"))
	));

	let mut duplicate = manifest();
	duplicate["tools"]
		.as_array_mut()
		.unwrap()
		.push(json!("assets.query"));
	assert!(
		matches!(load(&duplicate), Err(BundleError::DuplicateTool(id)) if id == "assets.query")
	);

	let mut missing_corpus = manifest();
	missing_corpus["corpora"].as_array_mut().unwrap().pop();
	assert!(matches!(
		load(&missing_corpus),
		Err(BundleError::MissingCorpus(CorpusId::ActiveProject))
	));

	let mut corpus = manifest();
	corpus["corpora"][0] = json!("../secrets");
	assert!(matches!(load(&corpus), Err(BundleError::InvalidJson(_))));
}

#[test]
fn discovery_and_invocation_share_the_live_profile_grants() {
	let bundle = Bundle::builtin().unwrap();
	assert!(bundle.instructions().contains("# Game Development Support"));
	assert_eq!(
		bundle.corpora(),
		&[CorpusId::EngineHelp, CorpusId::ActiveProject]
	);

	let service = LiveAdapter::new();
	let scope = &service.scope;
	let descriptors = bundle.available_tools(&service, scope);
	assert_eq!(
		descriptors.iter().map(|tool| tool.name).collect::<Vec<_>>(),
		["editor.inspect", "assets.query"]
	);
	assert!(
		descriptors
			.iter()
			.all(|tool| tool.read_only && !tool.description.is_empty())
	);
	let inspect_schema = serde_json::to_value(&descriptors[0].input_schema).unwrap();
	assert_eq!(inspect_schema["type"], "object");
	assert_eq!(inspect_schema["additionalProperties"], false);
	let asset_schema = serde_json::to_value(&descriptors[1].input_schema).unwrap();
	assert_eq!(asset_schema["required"], json!(["query"]));
	assert_eq!(asset_schema["properties"]["query"]["maxLength"], 512);
	assert_eq!(
		asset_schema["properties"]["limit"]["maximum"].as_f64(),
		Some(16.0)
	);

	assert_eq!(
		bundle
			.invoke(&service, scope, "editor.inspect", b"{}")
			.unwrap()["object"],
		"selected-prop"
	);
	assert_eq!(
		bundle
			.invoke(&service, scope, "assets.query", br#"{"query":"cube"}"#)
			.unwrap(),
		json!({"query": "cube", "limit": 8})
	);
	assert_eq!(
		bundle.invoke(
			&service,
			scope,
			"knowledge.search",
			br#"{"query":"gravity"}"#
		),
		Err(ToolError::Unavailable(ToolId::KnowledgeSearch))
	);
	assert_eq!(
		bundle.invoke(&service, scope, "scene.apply_batch", b"{}"),
		Err(ToolError::Denied)
	);

	let retired = ToolScope::new("opened-project".into(), 23, 9);
	assert!(bundle.available_tools(&service, &retired).is_empty());
	let foreign = ToolScope::new("another-project".into(), 24, 9);
	assert!(bundle.available_tools(&service, &foreign).is_empty());
	assert_eq!(
		bundle.invoke(&service, &foreign, "editor.inspect", b"{}"),
		Err(ToolError::Unavailable(ToolId::EditorInspect))
	);
	assert_eq!(service.calls.get(), 2);
}

#[test]
fn rejects_untyped_arguments_and_bounds_payloads_before_dispatch() {
	let bundle = Bundle::builtin().unwrap();
	let mut service = LiveAdapter::new();
	for invalid in [
		br#"{"query":"cube","project_id":"another-project"}"#.as_slice(),
		br#"{"query":"  "}"#,
		br#"{"query":"cube","limit":17}"#,
		br#"{"query":"cube","limit":0}"#,
		br#"{"query":"cube","query":"plane"}"#,
		br#"["cube",1]"#,
	] {
		assert_eq!(
			bundle.invoke(&service, &service.scope, "assets.query", invalid),
			Err(ToolError::InvalidArguments)
		);
	}
	assert_eq!(
		bundle.invoke(
			&service,
			&service.scope,
			"editor.inspect",
			br#"{"workspace_generation":8}"#
		),
		Err(ToolError::InvalidArguments)
	);
	assert_eq!(
		bundle.invoke(
			&service,
			&service.scope,
			"editor.inspect",
			&vec![b' '; 4097]
		),
		Err(ToolError::InvalidArguments)
	);
	assert_eq!(service.calls.get(), 0);
	service.oversized = true;
	assert_eq!(
		bundle.invoke(&service, &service.scope, "editor.inspect", b"{}"),
		Err(ToolError::ResultTooLarge)
	);
}
