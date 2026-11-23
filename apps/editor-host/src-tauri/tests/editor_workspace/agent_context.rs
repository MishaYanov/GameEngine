use super::*;
use bridge::viewport::PropPreviewTargetDto;
use editor::{
	agent_context::{
		AgentContextError, AgentContextErrorCode, AgentContextRequest, AgentObjectSummary,
		InspectorAddress,
	},
	prefab_sources::{nested::NestedPropPlacementDto, PropDraftEdit},
};
use runtime::{
	components::ComponentId,
	linked_prefab::{PlacementId, PropAddress},
	scene_data::TransformData,
};
use serde_json::json;

fn request(
	workspace: &EditorWorkspace,
	address: InspectorAddress,
	content_revision: u64,
) -> AgentContextRequest {
	AgentContextRequest {
		workspace_generation: workspace.generation,
		revision: workspace.backend.session().revision(),
		content_revision,
		address,
		component_id: None,
	}
}

fn scene_request(workspace: &EditorWorkspace, scene: SceneId) -> AgentContextRequest {
	request(
		workspace,
		InspectorAddress::Scene {
			scene,
			object: EntityKey::new("a").unwrap(),
		},
		workspace.backend.session().scene_revision(),
	)
}

fn query(
	workspace: &EditorWorkspace,
	request: AgentContextRequest,
) -> Result<AgentObjectSummary, AgentContextError> {
	workspace.capture_agent_context(request)?.resolve()
}

fn authored_state(workspace: &EditorWorkspace) -> (serde_json::Value, u64) {
	(
		serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap(),
		workspace.backend.change_cursor(),
	)
}

#[test]
fn scene_agent_context_uses_the_active_generation_and_revisions_without_mutating() {
	use AgentContextErrorCode::{Missing, Stale, Unsupported};

	let (fixture, project, second) = two_scene_project();
	let path = fixture.0.join("scenes/second.json");
	let mut same_name = runtime::scene_io::load_data(&path).unwrap().data;
	same_name.entities[0].name = "Same actor".into();
	same_name.entities[0].transform.position = [5.0, 0.0, 0.0];
	fs::write(path, same_name.to_json().unwrap()).unwrap();
	let scene_file = fixture.0.join("scenes/main.json");
	let on_disk = fs::read(&scene_file).unwrap();
	let mut workspace = fixture.open();
	run(
		&mut workspace,
		Command::Select {
			keys: vec!["a".into()],
		},
	);

	run(
		&mut workspace,
		Command::Rename {
			key: "a".into(),
			name: "Same actor".into(),
		},
	);

	let first_scene = project.startup_scene_id;
	let mut selected = scene_request(&workspace, first_scene.clone());
	selected.component_id = Some(ComponentId::new("mesh").unwrap());
	let before = authored_state(&workspace);
	assert_eq!(before.0["can_undo"], true);
	let summary = query(&workspace, selected.clone()).unwrap();
	assert_eq!(summary.name, "Same actor");
	assert_eq!(summary.entity_key.as_str(), "a");
	assert_eq!(summary.transform.position, [0.0; 3]);
	let wire = serde_json::to_value(summary).unwrap();
	assert_eq!(wire["selected_component"]["id"], "mesh");
	assert_eq!(
		wire["capabilities"],
		json!(["inspect_object", "inspect_component"])
	);
	assert_eq!(wire["content_revision"], selected.content_revision);
	assert_eq!(wire["address"]["scene"], first_scene.as_str());

	let mut stale = selected.clone();
	stale.workspace_generation += 1;
	assert_eq!(query(&workspace, stale).unwrap_err().code, Stale);
	let mut stale = selected.clone();
	stale.revision += 1;
	assert_eq!(query(&workspace, stale).unwrap_err().code, Stale);
	let mut stale = selected.clone();
	stale.content_revision += 1;
	assert_eq!(query(&workspace, stale).unwrap_err().code, Stale);
	let mut missing = selected.clone();
	missing.component_id = Some(ComponentId::new("unused").unwrap());
	assert_eq!(query(&workspace, missing).unwrap_err().code, Missing);
	assert_eq!(authored_state(&workspace), before);
	assert_eq!(fs::read(scene_file).unwrap(), on_disk);

	let other_fixture = Fixture::new();
	assert_eq!(
		query(&other_fixture.open(), selected).unwrap_err().code,
		Stale
	);
	run(
		&mut workspace,
		Command::SwitchScene {
			scene_id: second.clone(),
		},
	);
	assert_eq!(
		query(&workspace, scene_request(&workspace, first_scene))
			.unwrap_err()
			.code,
		Stale,
	);
	let summary = query(&workspace, scene_request(&workspace, second.clone())).unwrap();
	assert_eq!(summary.name, "Same actor");
	assert_eq!(summary.transform.position, [5.0, 0.0, 0.0]);

	run(
		&mut workspace,
		Command::BeginTransaction {
			label: "Uncommitted edit".into(),
		},
	);
	assert_eq!(
		query(&workspace, scene_request(&workspace, second.clone()))
			.unwrap_err()
			.code,
		Unsupported,
	);
	run(&mut workspace, Command::CancelTransaction);
	run(&mut workspace, Command::StartPlay);
	assert_eq!(
		query(&workspace, scene_request(&workspace, second))
			.unwrap_err()
			.code,
		Unsupported,
	);
}

#[test]
fn prop_agent_context_reads_the_pinned_unsaved_draft_without_advancing_preview() {
	use AgentContextErrorCode::{Missing, Stale};

	let fixture = Fixture::new();
	let mut workspace = fixture.open();
	run(
		&mut workspace,
		Command::Select {
			keys: vec!["a".into()],
		},
	);
	let revision = workspace.backend.session().revision();
	let draft = workspace
		.create_linked_prefab_source("prefabs/agent-source.json", Default::default(), revision)
		.unwrap();
	let path = fixture.0.join(&draft.path);
	let on_disk = fs::read(&path).unwrap();
	let preview = workspace
		.open_prop_preview(
			PropPreviewTargetDto::Source {
				draft_id: draft.draft_id.clone(),
			},
			revision,
			workspace.generation,
			1,
		)
		.unwrap();
	let mut prefab = draft.prefab.clone();
	prefab["entities"][0]["name"] = json!("Unsaved local source");
	let applied = workspace
		.edit_prop_draft(
			&draft.draft_id,
			draft.draft_revision,
			PropDraftEdit::Apply { prefab },
		)
		.unwrap();
	let wire = json!({
		"workspace_generation": workspace.generation,
		"revision": workspace.backend.session().revision(),
		"content_revision": applied.draft_revision,
		"address": { "kind": "prop", "draftId": draft.draft_id, "sourceObject": "a" }
	});
	let selected: AgentContextRequest = serde_json::from_value(wire.clone()).unwrap();
	let before = authored_state(&workspace);
	let capture = workspace.capture_agent_context(selected.clone()).unwrap();
	let summary = query(&workspace, selected.clone()).unwrap();
	assert_eq!(summary.name, "Unsaved local source");
	assert_eq!(
		serde_json::to_value(summary).unwrap()["address"],
		wire["address"]
	);
	assert_eq!(
		workspace.active_prop_preview().unwrap().state.token,
		preview.token
	);
	assert!(workspace.prop_preview_content_is_current(&preview.token, applied.draft_revision));
	assert_eq!(authored_state(&workspace), before);
	assert_eq!(fs::read(&path).unwrap(), on_disk);

	let undone = workspace
		.edit_prop_draft(&draft.draft_id, applied.draft_revision, PropDraftEdit::Undo)
		.unwrap();
	assert_eq!(capture.resolve().unwrap().name, "Unsaved local source");
	assert_eq!(query(&workspace, selected.clone()).unwrap_err().code, Stale);
	let current = request(&workspace, selected.address, undone.draft_revision);
	assert_eq!(query(&workspace, current.clone()).unwrap().name, "Original");
	workspace.close_prefab_source(&draft.draft_id).unwrap();
	assert_eq!(query(&workspace, current).unwrap_err().code, Missing);
	assert_eq!(fs::read(path).unwrap(), on_disk);
}

#[test]
fn linked_agent_context_resolves_the_same_semantic_member_under_the_requested_root() {
	use AgentContextErrorCode::{Missing, Unsupported};

	let fixture = Fixture::new();
	let mut workspace = fixture.open();
	run(
		&mut workspace,
		Command::Select {
			keys: vec!["a".into()],
		},
	);
	let revision = workspace.backend.session().revision();
	let outer = workspace
		.create_linked_prefab_source("prefabs/agent-parent.json", Default::default(), revision)
		.unwrap();
	let child = workspace
		.create_linked_prefab_source("prefabs/agent-child.json", Default::default(), revision)
		.unwrap();
	let source_entity = EntityKey::new("a").unwrap();
	let nested = workspace
		.add_prop_draft_placement(
			&outer.draft_id,
			outer.draft_revision,
			NestedPropPlacementDto {
				asset_id: child.asset_id,
				fingerprint: child.fingerprint,
				name: "Nested".into(),
				parent: source_entity.clone(),
				local: TransformData::default(),
				bindings: Default::default(),
			},
		)
		.unwrap();
	let placement = nested.prefab["composition"]["placements"][0]["id"]
		.as_str()
		.unwrap();
	let semantic = PropAddress::Object {
		placement_path: vec![PlacementId::new(placement).unwrap()],
		source_entity,
	};
	workspace
		.save_prop_draft(&outer.draft_id, nested.draft_revision)
		.unwrap();
	let mut roots = Vec::new();
	for position in [[0.0; 3], [5.0, 0.0, 0.0]] {
		let revision = workspace.backend.session().revision();
		let placed_draft = workspace.open_prefab_source(&outer.path, revision).unwrap();
		let placed = workspace
			.instantiate_linked_source(
				&outer.path,
				&placed_draft.draft_id,
				position,
				Default::default(),
				revision,
			)
			.unwrap();
		workspace
			.close_prefab_source(&placed_draft.draft_id)
			.unwrap();
		roots.push(EntityKey::new(&placed.selection[0]).unwrap());
	}
	let scene = workspace.backend.session().active_scene_id().clone();
	let content_revision = workspace.backend.session().scene_revision();
	let before = authored_state(&workspace);
	let mut keys = Vec::new();
	for root in &roots {
		let address = InspectorAddress::Linked {
			scene: scene.clone(),
			root: root.clone(),
			address: semantic.clone(),
		};
		let context = query(
			&workspace,
			request(&workspace, address.clone(), content_revision),
		)
		.unwrap();
		assert_eq!(context.address, address);
		assert_eq!(context.name, "Original");
		let resolved = workspace
			.linked_prop_address(
				context.entity_key.as_str(),
				workspace.generation,
				context.revision,
			)
			.unwrap()
			.unwrap();
		assert_eq!(&resolved.root_key, root);
		assert_eq!(resolved.address, semantic);
		keys.push(context.entity_key);
	}
	assert_ne!(keys[0], keys[1]);
	let wrong_path = InspectorAddress::Linked {
		scene: scene.clone(),
		root: roots[0].clone(),
		address: PropAddress::Placement {
			placement_path: vec![PlacementId::new("absent").unwrap()],
		},
	};
	assert_eq!(
		query(
			&workspace,
			request(&workspace, wrong_path, content_revision)
		)
		.unwrap_err()
		.code,
		Missing,
	);
	let flat_member = InspectorAddress::Scene {
		scene,
		object: keys.remove(0),
	};
	assert_eq!(
		query(
			&workspace,
			request(&workspace, flat_member, content_revision)
		)
		.unwrap_err()
		.code,
		Unsupported,
	);
	assert_eq!(authored_state(&workspace), before);
}

#[test]
fn agent_transport_errors_keep_distinct_machine_readable_codes() {
	for (code, label) in [
		(AgentContextErrorCode::Forbidden, "forbidden"),
		(AgentContextErrorCode::Unavailable, "unavailable"),
	] {
		let error = AgentContextError::new(code, "transport diagnostic");
		assert_eq!(serde_json::to_value(error).unwrap()["code"], label);
	}
}
