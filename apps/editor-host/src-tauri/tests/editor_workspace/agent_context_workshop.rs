use super::*;
use editor::agent_context::{AgentContextErrorCode, AgentContextRequest, InspectorAddress};
use runtime::{components::ComponentId, linked_prefab::PropAddress};

#[test]
fn workshop_preserves_authored_state_and_addresses_the_nested_component_exactly() {
	let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../headless/tests/fixtures/agent-context-workshop/project.json");
	let project = ProjectLocation::from_manifest(&manifest).unwrap();
	let south_file = project.assets().join("scenes/south.json");
	let saved = fs::read(&south_file).unwrap();
	let mut workspace = EditorWorkspace::open_project_with_notifier(project, || {}).unwrap();
	let north = workspace.backend.session().active_scene_id().clone();
	let repeated = EntityKey::new("status-sphere").unwrap();

	let north_context = workspace
		.capture_agent_context(AgentContextRequest {
			workspace_generation: workspace.generation,
			revision: workspace.backend.session().revision(),
			content_revision: workspace.backend.session().scene_revision(),
			address: InspectorAddress::Scene {
				scene: north.clone(),
				object: repeated.clone(),
			},
			component_id: Some(ComponentId::new("mesh").unwrap()),
		})
		.unwrap()
		.resolve()
		.unwrap();
	assert_eq!(north_context.name, "Signal Cap");
	assert_eq!(north_context.transform.position, [-2.0, 0.7, 0.0]);

	let south = SceneId::new("south").unwrap();
	run(
		&mut workspace,
		Command::SwitchScene {
			scene_id: south.clone(),
		},
	);
	let authored = workspace.backend.session().authored_document();
	let (root, link) = authored
		.entities
		.iter()
		.find_map(|entity| {
			entity
				.linked_prefab
				.as_ref()
				.map(|link| (entity.key.clone(), link))
		})
		.unwrap();
	let nested = PropAddress::Object {
		placement_path: vec![link.source.prefab.composition.placements[0].id.clone()],
		source_entity: repeated.clone(),
	};
	let exact_member = link.entity_map[&nested.materialized_key().unwrap()].clone();
	let before = serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap();
	let cursor = workspace.backend.change_cursor();
	let request = AgentContextRequest {
		workspace_generation: workspace.generation,
		revision: workspace.backend.session().revision(),
		content_revision: workspace.backend.session().scene_revision(),
		address: InspectorAddress::Linked {
			scene: south.clone(),
			root,
			address: nested,
		},
		component_id: Some(ComponentId::new("mesh").unwrap()),
	};
	let exact = workspace
		.capture_agent_context(request.clone())
		.unwrap()
		.resolve()
		.unwrap();
	assert_eq!(exact.address, request.address);
	assert_eq!(exact.entity_key, exact_member);
	assert_ne!(exact.entity_key, repeated);
	assert_eq!(exact.name, north_context.name);
	assert_eq!(exact.selected_component.unwrap().id.as_str(), "mesh");
	assert_eq!(workspace.backend.change_cursor(), cursor);
	assert_eq!(
		serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap(),
		before
	);
	assert_eq!(fs::read(south_file).unwrap(), saved);

	let wrong_scene = AgentContextRequest {
		address: InspectorAddress::Scene {
			scene: north,
			object: repeated.clone(),
		},
		..request.clone()
	};
	assert_eq!(
		workspace
			.capture_agent_context(wrong_scene)
			.err()
			.unwrap()
			.code,
		AgentContextErrorCode::Stale,
	);
	let flat = AgentContextRequest {
		address: InspectorAddress::Scene {
			scene: south,
			object: repeated,
		},
		..request
	};
	let same_name = workspace
		.capture_agent_context(flat)
		.unwrap()
		.resolve()
		.unwrap();
	assert_eq!(same_name.name, "Signal Cap");
	assert_eq!(same_name.transform.position, [2.0, 0.7, 0.0]);
	assert_ne!(same_name.entity_key, exact_member);
}
