use super::*;
use editor::prefab_sources::nested::NestedPropPlacementDto;
use runtime::{
	components::{ComponentId, NativeBehaviorComponent, NativeBehaviorSpec},
	editor::command::{TransformDto, TransformEditDto, TransformSpace},
	linked_prefab::NestedBinding,
	scene_data::{SceneData, TransformData},
	simulation_session::SimulationSession,
};
use serde::Deserialize;
use std::{collections::BTreeMap, sync::Arc};

const FLOOR_SOURCE: &str = "prefabs/floor.json";
const GRATE_SOURCE: &str = "prefabs/grate.json";
const RUNTIME_PREFAB: &str = "prefabs/spawnable-floor.json";
const RUNTIME_MESH: &str = "models/grate.glb";

#[derive(Deserialize)]
struct Layout {
	floor: TransformData,
	left: TransformData,
	right: TransformData,
	left_override: [f32; 3],
}

#[test]
fn nested_prop_roundtrips_from_editor_play_through_export() {
	let fixture = Fixture::new();
	let layout: Layout =
		serde_json::from_str(include_str!("../fixtures/prop-workspace/floor-layout.json")).unwrap();
	seed_source_objects(&fixture, layout.floor.clone());
	let mut workspace = fixture.open();
	let root = author_floor(&mut workspace, &layout);
	let revision = workspace.backend.session().revision();
	workspace.save_all(revision).unwrap();
	let scene_path = workspace.scene_path.clone();
	let saved = fs::read(&scene_path).unwrap();
	let manifest = workspace.project_path.clone();
	drop(workspace);

	let mut reopened = EditorWorkspace::open_project_with_notifier(
		ProjectLocation::from_manifest(&manifest).unwrap(),
		|| {},
	)
	.unwrap();
	let scene = reopened.backend.session().authored_document();
	let folder = &scene.organization.folders[0];
	assert_eq!(folder.name, "Floor Assemblies");
	assert_eq!(scene.organization.root_membership[&root], folder.id);
	let anchor = scene
		.entities
		.iter()
		.find(|entity| entity.key == root)
		.unwrap();
	let pinned = anchor.linked_prefab.as_ref().unwrap();
	assert_eq!(pinned.source.prefab.composition.placements.len(), 2);
	assert_eq!(pinned.source.prefab.composition.pins.len(), 1);
	let objects = pinned.entity_map.values().map(|key| {
		scene
			.entities
			.iter()
			.find(|entity| &entity.key == key)
			.unwrap()
	});
	let grates = objects
		.filter(|entity| entity.name == "Steel Grate")
		.collect::<Vec<_>>();
	assert_eq!(grates.len(), 2);
	assert!(grates.iter().all(|entity| matches!(
		&entity.native_behaviors[0].behavior,
		NativeBehaviorSpec::Collectible { actor, trigger, .. } if actor.as_str() == "actor" && trigger == &entity.key
	)));

	let floor_source = fs::read(fixture.0.join(FLOOR_SOURCE)).unwrap();
	let child_source = fs::read(fixture.0.join(GRATE_SOURCE)).unwrap();
	fs::write(fixture.0.join(RUNTIME_PREFAB), &floor_source).unwrap();
	run(&mut reopened, Command::StartPlay);
	assert!(reopened.backend.session().is_playing());
	let play = reopened.backend.session().render_document();
	assert_runtime_parity(&scene, &play);
	run(&mut reopened, Command::StopPlay);
	assert_eq!(
		fs::read(fixture.0.join(FLOOR_SOURCE)).unwrap(),
		floor_source
	);
	assert_eq!(
		fs::read(fixture.0.join(GRATE_SOURCE)).unwrap(),
		child_source
	);
	assert_eq!(fs::read(&scene_path).unwrap(), saved);

	fs::remove_file(fixture.0.join(FLOOR_SOURCE)).unwrap();
	fs::remove_file(fixture.0.join(GRATE_SOURCE)).unwrap();
	let prepared = reopened
		.prepare_build(reopened.backend.session().revision())
		.unwrap();
	let destination = fixture.0.parent().unwrap().join("export");
	fs::create_dir(&destination).unwrap();
	let staged =
		compiler::stage_prepared_project(&prepared, destination.to_str().unwrap()).unwrap();
	let package = staged.path();
	assert!(!package.join("assets").join(FLOOR_SOURCE).exists());
	assert!(!package.join("assets").join(GRATE_SOURCE).exists());
	let asset_root = package.join("assets");
	let spawnable = runtime::editor::PrefabData::from_json(
		&fs::read_to_string(asset_root.join(RUNTIME_PREFAB)).unwrap(),
	)
	.unwrap();
	assert!(spawnable.composition.is_empty());
	assert_eq!(spawnable.entities.len(), 5);
	assert_eq!(spawnable.reference_slots.get("Player"), Some(&key("actor")));
	let project = runtime::project::ProjectData::load(package.join("project.json")).unwrap();
	let exported = runtime::scene_io::load_data(
		&asset_root.join(project.scene_path(&project.startup_scene_id).unwrap()),
	)
	.unwrap()
	.data;
	assert_runtime_parity(&scene, &exported);
	let mut headless = SimulationSession::new(project.clone(), Arc::new(exported)).unwrap();
	headless.tick(0.05).unwrap();
	assert_runtime_parity(&scene, &headless.document());

	// Exercise platform rendering when the matching Player binary is supplied.
	if let Some(binary) = std::env::var_os("ENGINE_TEST_PROP_PLAYER") {
		let executable = package.join(format!("game{}", std::env::consts::EXE_SUFFIX));
		fs::copy(binary, &executable).unwrap();
		let launch_from = fixture.0.parent().unwrap().join("player-working-directory");
		fs::create_dir(&launch_from).unwrap();
		let output = std::process::Command::new(executable)
			.current_dir(launch_from)
			.arg("--smoke-test")
			.output()
			.unwrap();
		assert!(
			output.status.success(),
			"Installed Player smoke failed: {}",
			String::from_utf8_lossy(&output.stderr)
		);
	}

	assert_ne!(asset_root, fixture.0);
	let archived_mesh = fixture.0.join(RUNTIME_MESH);
	assert_eq!(
		fs::read(asset_root.join(RUNTIME_MESH)).unwrap(),
		fs::read(&archived_mesh).unwrap()
	);
	fs::remove_file(archived_mesh).unwrap();
	let error = compiler::prepare_project(&project, &fixture.0)
		.err()
		.unwrap();
	assert!(
		error.contains("scenes/main.json") && error.contains(RUNTIME_MESH),
		"{error}"
	);
	assert!(
		grates
			.iter()
			.any(|entity| error.contains(entity.key.as_str())),
		"{error}"
	);
	assert_eq!(fs::read(&scene_path).unwrap(), saved);

	let mut defective: serde_json::Value = serde_json::from_slice(&saved).unwrap();
	replace_text(&mut defective, "actor", "missing-actor");
	let units = defective["entities"].as_array_mut().unwrap();
	units.retain(|entity| entity["key"] != "missing-actor");
	fs::write(&scene_path, serde_json::to_vec(&defective).unwrap()).unwrap();
	let error = compiler::prepare_project(&project, &fixture.0)
		.err()
		.unwrap();
	assert!(
		error.contains("scenes/main.json") && error.contains("missing-actor"),
		"{error}"
	);
	assert!(
		grates
			.iter()
			.any(|entity| error.contains(entity.key.as_str())),
		"{error}"
	);
}

fn seed_source_objects(fixture: &Fixture, floor_transform: TransformData) {
	let path = fixture.0.join("scenes/main.json");
	fs::copy(
		fixture.0.join("models/triangle.glb"),
		fixture.0.join(RUNTIME_MESH),
	)
	.unwrap();
	let mut data = runtime::scene_io::load_data(&path).unwrap().data;
	data.entities[0].name = "Floor".into();
	data.entities[0].transform = floor_transform;
	let mut grate = EntityData::new(key("grate"), "Steel Grate", RUNTIME_MESH);
	grate.native_behaviors.push(NativeBehaviorComponent {
		id: ComponentId::generate(),
		enabled: false,
		behavior: NativeBehaviorSpec::Collectible {
			actor: key("actor"),
			trigger: key("grate"),
			visual: None,
			despawn_owner: false,
		},
	});
	data.entities.push(EntityData::new(
		key("actor"),
		"Actor",
		"models/triangle.glb",
	));
	data.entities.push(grate);
	fs::write(path, data.to_json().unwrap()).unwrap();
}

fn author_floor(workspace: &mut EditorWorkspace, layout: &Layout) -> EntityKey {
	run(
		workspace,
		Command::Select {
			keys: vec!["a".into()],
		},
	);
	let revision = workspace.backend.session().revision();
	let floor_slots = BTreeMap::from([("Player".to_owned(), key("actor"))]);
	let floor = workspace
		.create_linked_prefab_source(FLOOR_SOURCE, floor_slots.clone(), revision)
		.unwrap();
	run(
		workspace,
		Command::Select {
			keys: vec!["grate".into()],
		},
	);
	let revision = workspace.backend.session().revision();
	let slot = BTreeMap::from([("User".to_owned(), key("actor"))]);
	let grate = workspace
		.create_linked_prefab_source(GRATE_SOURCE, slot, revision)
		.unwrap();
	let add = |name: &str, local| NestedPropPlacementDto {
		asset_id: grate.asset_id.clone(),
		fingerprint: grate.fingerprint.clone(),
		name: name.into(),
		parent: key("a"),
		local,
		bindings: BTreeMap::from([(
			"User".to_owned(),
			NestedBinding::Slot {
				name: "Player".into(),
			},
		)]),
	};
	let left = workspace
		.add_prop_draft_placement(
			&floor.draft_id,
			floor.draft_revision,
			add("Left Grate", layout.left.clone()),
		)
		.unwrap();
	let both = workspace
		.add_prop_draft_placement(
			&floor.draft_id,
			left.draft_revision,
			add("Right Grate", layout.right.clone()),
		)
		.unwrap();
	workspace
		.save_prop_draft(&floor.draft_id, both.draft_revision)
		.unwrap();
	let floor = workspace
		.open_prefab_source(FLOOR_SOURCE, revision)
		.unwrap();
	let placed = workspace
		.instantiate_linked_source(
			FLOOR_SOURCE,
			&floor.draft_id,
			[4.0, 0.0, 2.0],
			floor_slots,
			revision,
		)
		.unwrap();
	let root = key(&placed.selection[0]);
	run(
		workspace,
		Command::Select {
			keys: vec!["grate".into()],
		},
	);
	run(workspace, Command::DeleteSelection);
	let document = workspace.backend.session().authored_document();
	let left = document
		.entities
		.iter()
		.find(|entity| entity.name == "Left Grate")
		.unwrap();
	run(
		workspace,
		Command::SetTransforms {
			edits: vec![TransformEditDto {
				key: left.key.as_str().into(),
				transform: TransformDto {
					position: layout.left_override,
					..TransformDto::default()
				},
			}],
			space: TransformSpace::Local,
			snap: false,
		},
	);
	run(
		workspace,
		Command::CreateFolder {
			name: "Floor Assemblies".into(),
			parent: None,
		},
	);
	let document = workspace.backend.session().authored_document();
	run(
		workspace,
		Command::MoveRootsToFolder {
			roots: vec![root.clone()],
			folder: Some(document.organization.folders[0].id.clone()),
		},
	);
	root
}

fn assert_runtime_parity(authored: &SceneData, runtime: &SceneData) {
	assert!(runtime.organization.is_empty());
	assert!(runtime
		.entities
		.iter()
		.all(|entity| entity.linked_prefab.is_none()));
	assert_eq!(
		authored.world_matrices().unwrap(),
		runtime.world_matrices().unwrap()
	);
	assert_eq!(runtime::linked_prefab::resolve_scene(authored), *runtime);
}

fn replace_text(value: &mut serde_json::Value, old: &str, new: &str) {
	match value {
		serde_json::Value::String(value) if value == old => *value = new.to_owned(),
		serde_json::Value::Array(items) => items
			.iter_mut()
			.for_each(|item| replace_text(item, old, new)),
		serde_json::Value::Object(fields) => fields
			.values_mut()
			.for_each(|item| replace_text(item, old, new)),
		_ => {}
	}
}

fn key(value: &str) -> EntityKey {
	EntityKey::new(value).unwrap()
}
