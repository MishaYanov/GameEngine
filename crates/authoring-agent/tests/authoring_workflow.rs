use authoring_agent::{
	CorpusId,
	knowledge::{
		ApprovedSource, Compatibility, CorpusManifest, KnowledgeIndex, ReferenceHit, SearchScope,
		lexical_search,
	},
	session::{
		AgentRequest, CancellationToken, FinishStatus, ReferenceProvider, RequestScope, Session,
		WorkerEventKind, WorkerInput, WorkerScope,
	},
};
use std::{path::Path, sync::mpsc, time::Duration};

const PROJECT_ID: &str = "agent-context-workshop";
const SCHEMA_VERSION: u32 = 1;

struct References {
	index: KnowledgeIndex,
	scope: SearchScope,
}

impl ReferenceProvider for References {
	fn search(&mut self, question: &str, cancel: &CancellationToken) -> Vec<ReferenceHit> {
		if cancel.is_cancelled() {
			return Vec::new();
		}
		lexical_search(&self.index, &self.scope, question, 8)
	}
}

#[test]
fn offline_workshop_guidance_cites_current_sources_in_the_active_project() {
	let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
	let help = workspace.join("docs");
	let project = workspace.join("apps/headless/tests/fixtures/agent-context-workshop");
	let compatibility = Compatibility::new(env!("CARGO_PKG_VERSION"), SCHEMA_VERSION);
	let manifest = CorpusManifest::new(
		compatibility.clone(),
		&help,
		Some((PROJECT_ID, &project)),
		vec![
			ApprovedSource::new(
				CorpusId::EngineHelp,
				"editor-workspace",
				"editor-workspace.md",
			)
			.unwrap(),
			ApprovedSource::new(
				CorpusId::ActiveProject,
				"signal-cap",
				"assets/prefabs/signal-cap.prefab.json",
			)
			.unwrap(),
		],
	)
	.unwrap();
	let mut index = KnowledgeIndex::default();
	index.refresh(&manifest).unwrap();
	let mut initial = Vec::new();
	index
		.write_snapshot(manifest.scope(), &mut initial)
		.unwrap();
	let mut refreshed = Vec::new();
	index.refresh(&manifest).unwrap();
	index
		.write_snapshot(manifest.scope(), &mut refreshed)
		.unwrap();
	assert_eq!(initial, refreshed);

	let sibling = SearchScope::new(compatibility, Some("different-project".into()));
	assert!(
		lexical_search(&index, &sibling, "Signal Cap", 8)
			.iter()
			.all(|hit| hit.citation.project_id.is_none())
	);
	let incompatible = SearchScope::new(
		Compatibility::new(env!("CARGO_PKG_VERSION"), SCHEMA_VERSION + 1),
		Some(PROJECT_ID.into()),
	);
	assert!(lexical_search(&index, &incompatible, "Signal Cap", 8).is_empty());

	let (send, events) = mpsc::channel();
	let session = Session::new(
		WorkerScope::new(PROJECT_ID.into(), 2).unwrap(),
		References {
			index,
			scope: manifest.scope().clone(),
		},
		None,
		move |event| {
			send.send(event).unwrap();
		},
	)
	.unwrap();
	session.handle(WorkerInput::Start {
		request: Box::new(AgentRequest {
			id: "workshop-offline".into(),
			scope: RequestScope {
				project_id: PROJECT_ID.into(),
				workspace_generation: 2,
				revision: 4,
			},
			question: "Signal Cap".into(),
			selection: None,
		}),
	});
	let next = || {
		let event = events.recv_timeout(Duration::from_secs(4)).unwrap();
		assert_eq!(event.id, "workshop-offline");
		event.kind
	};
	assert!(matches!(next(), WorkerEventKind::Started));
	let WorkerEventKind::Sources { citations } = next() else {
		panic!("expected source citations")
	};
	let hit = citations
		.iter()
		.find(|hit| hit.citation.source_id == "signal-cap")
		.unwrap();
	assert_eq!(hit.citation.project_id.as_deref(), Some(PROJECT_ID));
	assert_eq!(
		hit.citation.relative_path,
		"assets/prefabs/signal-cap.prefab.json"
	);
	assert_eq!(
		hit.citation.compatibility.tool_schema_version,
		SCHEMA_VERSION
	);
	assert!(hit.excerpt.contains("Signal Cap"));
	assert!(matches!(
		next(),
		WorkerEventKind::Finished {
			status: FinishStatus::RetrievalOnly,
			..
		}
	));
}
