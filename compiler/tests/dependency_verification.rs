use compiler::{DependencyOpen, DependencySeverity, inspect_scene_dependencies, prepare_project};
use engine_core::{
	EntityKey, Key, SceneData,
	asset_catalog::{AssetCatalog, AssetId, AssetKind, MeshSignature, PersistentAsset},
	character_controller::{CharacterControllerComponent, CharacterControllerSettings},
	command::{EditorCommandDto, EditorCommandRequest},
	components::{
		ComponentData, ComponentId, ComponentRecord, NO_MESH_PATH, NativeBehaviorComponent,
		NativeBehaviorSpec, ScriptComponent, ScriptReference, ScriptSettings, SkyBackground,
		SkySettings,
	},
	editor::{EditorSession, PrefabData},
	input::{GameInputBinding, InputProfile},
	linked_prefab::{LinkedPrefabSource, NestedPlacement, PinnedAssetVersion, PlacementId},
	project::{PhysicsLayer, PhysicsLayers, ProjectData},
	scene_data::{
		CURRENT_SCHEMA_VERSION, ColliderData, ColliderInteraction, EntityData, TransformData,
	},
};
use sha2::{Digest, Sha256};
use std::{
	fs,
	path::{Path, PathBuf},
	sync::Arc,
};

struct Fixture(PathBuf);

impl Fixture {
	fn new() -> Self {
		let root = std::env::temp_dir().join(format!(
			"engine-dependency-audit-{}",
			AssetId::generate().as_str()
		));
		fs::create_dir_all(root.join("assets/scenes")).unwrap();
		Self(root)
	}
}

impl Drop for Fixture {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

#[test]
fn script_diagnostics_match_play_and_package_preflight_until_repaired() {
	let fixture = Fixture::new();
	let root = fixture.0.join("assets");
	let object = |name| {
		EntityData::new(EntityKey::new(name).unwrap(), name, NO_MESH_PATH).with_renderable(false)
	};
	let mut owner = object("door");
	let mut sensor = object("sensor");
	let id = ComponentId::new("door-control").unwrap();
	owner.scripts.push(ScriptComponent {
		id: id.clone(),
		enabled: true,
		settings: ScriptSettings {
			script_id: "project.removed_script".into(),
			config_version: 1,
			properties: Default::default(),
			references: [(
				"sensor".into(),
				ScriptReference::Entity {
					entity: sensor.key.clone(),
				},
			)]
			.into(),
		},
	});
	let mut scene = SceneData {
		schema_version: CURRENT_SCHEMA_VERSION,
		entities: vec![owner, sensor.clone()],
		camera: Default::default(),
		lights: vec![],
		bindings: Default::default(),
		organization: Default::default(),
		default_game_camera: None,
		environment: Default::default(),
	};
	let mut project =
		ProjectData::for_scene("Door diagnostics", "scenes/main.json", &scene).unwrap();
	project.input_profile = Some(InputProfile {
		actions: vec![GameInputBinding {
			id: "jump".into(),
			key: Key::Space,
			animation_parameter: None,
		}],
		..Default::default()
	});
	let factory = gameplay_catalog::resolve(&project).unwrap();
	let diagnose = |scene: &SceneData, code: &str| {
		fs::write(root.join("scenes/main.json"), scene.to_json().unwrap()).unwrap();
		let issues =
			inspect_scene_dependencies(&project, &project.startup_scene_id, scene, &root).unwrap();
		assert_eq!(issues.len(), 1);
		assert_eq!(issues[0].code, code);
		assert_eq!(
			issues[0].owner.as_ref().unwrap().component_id.as_ref(),
			Some(&id)
		);
		assert!(matches!(
			issues[0].open,
			Some(DependencyOpen::Script { .. })
		));
		let play_error = engine_core::simulation_session::SimulationSession::with_factory(
			project.clone(),
			Arc::new(scene.clone()),
			factory.as_ref(),
		)
		.err()
		.unwrap();
		assert_eq!(issues[0].message, play_error);
		let build_error = prepare_project(&project, &root).err().unwrap();
		assert!(build_error.contains(&play_error), "{build_error}");
	};
	diagnose(&scene, "missing_script");
	scene.entities[0].scripts[0].settings.script_id = "project.open_door_on_trigger".into();
	diagnose(&scene, "script_reference_invalid");
	sensor = sensor.with_collider(Some(ColliderData::Box {
		half_extents: [0.5; 3],
		offset: [0.; 3],
		interaction: ColliderInteraction {
			sensor: true,
			..Default::default()
		},
	}));
	scene.entities[1] = sensor;
	scene.entities[0] = scene.entities[0]
		.clone()
		.with_collider(Some(ColliderData::Box {
			half_extents: [0.3, 0.9, 0.3],
			offset: [0.; 3],
			interaction: Default::default(),
		}));
	scene.entities[0].character_controller = Some(CharacterControllerComponent {
		id: ComponentId::new("door-character").unwrap(),
		enabled: true,
		settings: CharacterControllerSettings::default(),
	});
	diagnose(&scene, "script_configuration_invalid");
	scene.entities[0]
		.character_controller
		.as_mut()
		.unwrap()
		.enabled = false;
	fs::write(root.join("scenes/main.json"), scene.to_json().unwrap()).unwrap();
	assert!(
		inspect_scene_dependencies(&project, &project.startup_scene_id, &scene, &root)
			.unwrap()
			.is_empty()
	);
	let package = prepare_project(&project, &root).unwrap();
	let staged = compiler::stage_prepared_project(&package, fixture.0.to_str().unwrap()).unwrap();
	let staged_scene = SceneData::from_json(
		&fs::read_to_string(staged.path().join("assets/scenes/main.json")).unwrap(),
	)
	.unwrap();
	assert_eq!(staged_scene.entities[0].scripts, scene.entities[0].scripts);
	assert!(
		engine_core::simulation_session::SimulationSession::with_factory(
			project,
			Arc::new(staged_scene),
			factory.as_ref(),
		)
		.is_ok()
	);
}

#[test]
fn resolves_pinned_members_and_reports_broken_animation_and_panorama_owners() {
	let fixture = Fixture::new();
	let root = fixture.0.join("assets");
	let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("../apps/game/assets");
	let scene = SceneData::from_json(
		&fs::read_to_string(sample.join("scenes/animation-room.json")).unwrap(),
	)
	.unwrap();
	let mut actor = scene
		.entities
		.iter()
		.find(|actor| actor.key.as_str() == "clip-demo")
		.unwrap()
		.clone();
	actor.parent = None;
	let component = actor.animation.as_ref().unwrap().id.clone();
	let asset_id = actor
		.animation
		.as_ref()
		.unwrap()
		.settings
		.source
		.asset_id
		.clone();
	let catalog =
		AssetCatalog::from_json(&fs::read_to_string(sample.join("asset-catalog.json")).unwrap())
			.unwrap();
	let model = catalog.by_id(&asset_id).unwrap().clone();
	let sky_id = AssetId::generate();
	let png = include_bytes!("fixtures/sky-panorama-4x2.png");
	let panorama = PersistentAsset {
		id: sky_id.clone(),
		path: "panorama.png".into(),
		kind: AssetKind::SkyPanorama,
		revision: 1,
		fingerprint: format!("{:x}", Sha256::digest(png)),
		bytes: png.len() as u64,
		aliases: vec![],
		source_path: None,
		meshes: vec![],
		dependencies: vec![],
	};
	fs::write(root.join("panorama.png"), png).unwrap();

	let input = root.join(&model.path);
	fs::create_dir_all(input.parent().unwrap()).unwrap();
	fs::copy(sample.join(&model.path), &input).unwrap();
	fs::write(
		root.join("asset-catalog.json"),
		AssetCatalog {
			schema_version: 1,
			assets: vec![model.clone(), panorama],
		}
		.to_json()
		.unwrap(),
	)
	.unwrap();

	let leaf = linked_source("prefabs/performer.json", actor);
	let frame = EntityKey::new("stand").unwrap();
	let mut assembly = linked_source(
		"prefabs/stage.json",
		EntityData::new(frame.clone(), "Stand", NO_MESH_PATH).with_renderable(false),
	);
	assembly.prefab = assembly
		.prefab
		.with_nested_placement(
			&assembly.source_id,
			&leaf,
			NestedPlacement {
				id: PlacementId::new("performer").unwrap(),
				name: "Performer".into(),
				parent: frame,
				local: TransformData::default(),
				target: PinnedAssetVersion {
					asset_id: leaf.source_id.clone(),
					fingerprint: leaf.fingerprint.clone(),
				},
				bindings: Default::default(),
			},
		)
		.unwrap();
	assembly.fingerprint = format!(
		"{:x}",
		Sha256::digest(assembly.prefab.to_json().unwrap().as_bytes())
	);

	let mut authored = scene;
	authored.entities.clear();
	authored.bindings.clear();
	authored.camera = Default::default();
	authored.default_game_camera = None;
	let sky_key = EntityKey::new("yard-sky").unwrap();
	let sky_component = ComponentId::new("panorama-owner").unwrap();
	let mut sky = EntityData::new(sky_key.clone(), "Yard Sky", NO_MESH_PATH).with_renderable(false);
	sky.put_component(
		ComponentRecord {
			id: sky_component.clone(),
			schema_version: 1,
			enabled: true,
			data: ComponentData::Sky {
				settings: SkySettings {
					background: SkyBackground::Panorama {
						image: sky_id.clone(),
					},
					..Default::default()
				},
			},
		},
		false,
	)
	.unwrap();
	authored.environment.active_sky = Some(sky_key.clone());
	authored.entities.push(sky);

	let project =
		ProjectData::for_scene("Dependency verification", "scenes/main.json", &authored).unwrap();
	let mut session = EditorSession::for_project(authored, project.clone()).unwrap();
	session
		.instantiate_linked_prefab(assembly, [0.0; 3], Default::default())
		.unwrap();
	let scene = session.authored_document();
	let anchor = scene
		.entities
		.iter()
		.find(|entity| entity.linked_prefab.is_some())
		.unwrap();
	let actor = scene
		.entities
		.iter()
		.find(|entity| entity.animation.is_some())
		.unwrap();
	fs::write(root.join("scenes/main.json"), scene.to_json().unwrap()).unwrap();
	let check =
		|| inspect_scene_dependencies(&project, &project.startup_scene_id, &scene, &root).unwrap();
	let warnings = check();
	assert_eq!(warnings.len(), 2);
	assert!(
		warnings
			.iter()
			.all(|issue| issue.severity == DependencySeverity::Warning)
	);
	assert!(prepare_project(&project, &root).is_ok());

	fs::remove_file(&input).unwrap();
	fs::remove_file(root.join("panorama.png")).unwrap();
	let problems = check();
	let sky = problems
		.iter()
		.find(|issue| issue.code == "sky_panorama_invalid")
		.unwrap();
	assert_eq!(sky.severity, DependencySeverity::Error);
	assert_eq!(sky.owner.as_ref().unwrap().key, sky_key);
	assert_eq!(
		sky.owner.as_ref().unwrap().component_id.as_ref(),
		Some(&sky_component)
	);
	assert_eq!(
		sky.target.as_ref().unwrap().asset_id.as_ref(),
		Some(&sky_id)
	);
	assert!(matches!(sky.open, Some(DependencyOpen::Inspector)));

	let problem = problems
		.iter()
		.find(|issue| issue.code == "animation_source_invalid")
		.unwrap();
	assert_eq!(problem.severity, DependencySeverity::Error);
	assert_eq!(problem.owner.as_ref().unwrap().key, actor.key);
	assert_eq!(
		problem.owner.as_ref().unwrap().component_id.as_ref(),
		Some(&component)
	);
	assert_eq!(
		problem.target.as_ref().unwrap().asset_id.as_ref(),
		Some(&asset_id)
	);
	assert_eq!(
		problem.target.as_ref().unwrap().path.as_deref(),
		Some(model.path.as_str())
	);
	assert!(
		matches!(problem.open.as_ref(), Some(DependencyOpen::Animation {key}) if key.as_ref() == Some(&actor.key))
	);
	assert_eq!(
		problems
			.iter()
			.filter(|issue| issue.severity == DependencySeverity::Warning
				&& issue
					.owner
					.as_ref()
					.is_some_and(|owner| owner.key == anchor.key))
			.count(),
		2
	);
	assert!(
		prepare_project(&project, &root)
			.err()
			.unwrap()
			.contains(sky_key.as_str())
	);
	fs::write(root.join("panorama.png"), png).unwrap();
	assert!(
		prepare_project(&project, &root)
			.err()
			.unwrap()
			.contains(actor.key.as_str())
	);
}

#[test]
fn catalog_cycles_are_blocked_without_looping_or_losing_the_owner() {
	let fixture = Fixture::new();
	let root = fixture.0.join("assets");
	let first = AssetId::generate();
	let second = AssetId::generate();
	let contents = b"cycle sentinel";
	let row = |id: AssetId, path: &str, dependency: AssetId| PersistentAsset {
		id,
		path: path.into(),
		kind: AssetKind::Model,
		revision: 1,
		fingerprint: format!("{:x}", Sha256::digest(contents)),
		bytes: contents.len() as u64,
		aliases: vec![],
		source_path: None,
		meshes: vec![MeshSignature {
			index: 0,
			name: "mesh".into(),
			primitives: 1,
			identity: None,
		}],
		dependencies: vec![dependency],
	};
	let rows = vec![
		row(first.clone(), "first.glb", second.clone()),
		row(second, "second.glb", first.clone()),
	];
	for asset in &rows {
		fs::write(root.join(&asset.path), contents).unwrap();
	}
	fs::write(
		root.join("asset-catalog.json"),
		AssetCatalog {
			schema_version: 1,
			assets: rows,
		}
		.to_json()
		.unwrap(),
	)
	.unwrap();
	let mut scene = SceneData::from_json(
		&fs::read_to_string(
			Path::new(env!("CARGO_MANIFEST_DIR"))
				.join("../apps/game/assets/scenes/animation-room.json"),
		)
		.unwrap(),
	)
	.unwrap();
	scene.entities = vec![
		EntityData::new(
			EntityKey::new("cycle-owner").unwrap(),
			"Cycle owner",
			"first.glb",
		)
		.with_asset_id(Some(first)),
	];
	scene.camera = Default::default();
	scene.default_game_camera = None;
	scene.bindings.clear();
	let project = ProjectData::for_scene("Catalog cycle", "scenes/main.json", &scene).unwrap();
	fs::write(root.join("scenes/main.json"), scene.to_json().unwrap()).unwrap();
	let issues =
		inspect_scene_dependencies(&project, &project.startup_scene_id, &scene, &root).unwrap();
	assert_eq!(issues.len(), 1);
	assert_eq!(
		issues[0].owner.as_ref().unwrap().key.as_str(),
		"cycle-owner"
	);
	assert!(
		issues[0].message.contains("Cyclic asset dependency"),
		"{}",
		issues[0].message
	);
	assert!(
		prepare_project(&project, &root)
			.err()
			.unwrap()
			.contains("Cyclic asset dependency")
	);
}

#[test]
fn linked_collider_layer_override_reports_optional_and_required_contacts_during_preflight() {
	let fixture = Fixture::new();
	let root = fixture.0.join("assets");
	let scene = SceneData {
		schema_version: CURRENT_SCHEMA_VERSION,
		organization: Default::default(),
		default_game_camera: None,
		environment: Default::default(),
		bindings: Default::default(),
		entities: vec![],
		lights: vec![],
		camera: Default::default(),
	};
	let mut project = ProjectData::for_scene("Linked Sensor", "scenes/main.json", &scene).unwrap();
	project.physics_layers = Some(PhysicsLayers {
		layers: vec![PhysicsLayer {
			id: "trigger".into(),
			label: "Trigger".into(),
			bit: 31,
			archived: false,
		}],
		ignored_pairs: vec![[0, 31]],
	});
	let mut editor = EditorSession::for_project(scene, project).unwrap();
	apply(&mut editor, EditorCommandDto::CreatePlayer);
	let actor = editor.selection()[0].clone();
	apply(
		&mut editor,
		EditorCommandDto::SetLocalPlayer {
			key: Some(actor.clone()),
		},
	);

	let collider = |layers| ColliderData::Box {
		half_extents: [0.5; 3],
		offset: [0.0; 3],
		interaction: ColliderInteraction {
			sensor: true,
			layers,
			..Default::default()
		},
	};
	let source = linked_source(
		"prefabs/sensor.json",
		EntityData::new(EntityKey::new("sensor").unwrap(), "Sensor", NO_MESH_PATH)
			.with_renderable(false)
			.with_collider(Some(collider(1))),
	);
	editor
		.instantiate_linked_prefab(source, [0.0; 3], Default::default())
		.unwrap();
	let instantiated = editor.authored_document();
	let sensor = instantiated
		.entities
		.iter()
		.find(|entity| {
			entity
				.collider
				.as_ref()
				.is_some_and(|collider| collider.interaction().sensor)
		})
		.unwrap()
		.key
		.clone();
	let anchor = instantiated
		.entities
		.iter()
		.find(|entity| entity.linked_prefab.is_some())
		.unwrap()
		.key
		.clone();
	apply(
		&mut editor,
		EditorCommandDto::SetCollider {
			key: sensor.as_str().into(),
			collider: Some(collider(1 << 31)),
		},
	);
	let project = editor.project();
	let mut scene = (*editor.authored_document()).clone();
	let inspect = |scene: &SceneData| {
		inspect_scene_dependencies(project, &project.startup_scene_id, scene, &root).unwrap()
	};
	let persist = |scene: &SceneData| {
		fs::write(root.join("scenes/main.json"), scene.to_json().unwrap()).unwrap()
	};
	persist(&scene);

	let optional = inspect(&scene)
		.into_iter()
		.find(|issue| issue.code == "sensor_player_pair_ignored")
		.unwrap();
	assert_eq!(optional.severity, DependencySeverity::Warning);
	assert!(matches!(&optional.open, Some(DependencyOpen::Prop { key }) if key == &anchor));
	assert!(prepare_project(project, &root).is_ok());

	let owner = scene
		.entities
		.iter_mut()
		.find(|entity| entity.key == actor)
		.unwrap();
	let component_id = ComponentId::new("pickup").unwrap();
	owner.native_behaviors.push(NativeBehaviorComponent {
		id: component_id.clone(),
		enabled: true,
		behavior: NativeBehaviorSpec::Collectible {
			actor,
			trigger: sensor.clone(),
			visual: None,
			despawn_owner: false,
		},
	});
	persist(&scene);
	let required = inspect(&scene)
		.into_iter()
		.find(|issue| issue.code == "required_physics_pair_ignored")
		.unwrap();
	assert_eq!(required.severity, DependencySeverity::Error);
	assert_eq!(required.owner.unwrap().component_id, Some(component_id));
	assert_eq!(required.target.unwrap().key, Some(sensor));
	assert!(
		prepare_project(project, &root)
			.err()
			.unwrap()
			.contains(&required.message)
	);
}

fn apply(editor: &mut EditorSession, command: EditorCommandDto) {
	editor
		.execute_command(EditorCommandRequest {
			expected_revision: editor.revision(),
			command,
		})
		.unwrap();
}

fn linked_source(path: &str, entity: EntityData) -> LinkedPrefabSource {
	let prefab = PrefabData {
		schema_version: 3,
		entities: vec![entity],
		gameplay: None,
		reference_slots: Default::default(),
		composition: Default::default(),
	};
	let fingerprint = format!("{:x}", Sha256::digest(prefab.to_json().unwrap().as_bytes()));
	LinkedPrefabSource {
		source_id: AssetId::generate(),
		path: path.into(),
		fingerprint,
		prefab,
	}
}
