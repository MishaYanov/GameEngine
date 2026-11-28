use super::*;
use std::collections::BTreeMap;
struct Fixture(std::path::PathBuf);
impl Drop for Fixture {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.0);
	}
}
fn fixture() -> (Fixture, EditorWorkspace) {
	let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../../target")
		.join(format!("m5-prefab-source-{}", AssetId::generate().as_str()));
	for path in ["assets/props", "assets/scenes", "assets/prefabs"] {
		std::fs::create_dir_all(root.join(path)).unwrap();
	}
	std::fs::copy(
		std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../game/assets/props/cube.glb"),
		root.join("assets/props/cube.glb"),
	)
	.unwrap();
	let mut scene =
		runtime::SceneData::from_json(include_str!("../../../../game/assets/scenes/robots.json"))
			.unwrap();
	scene.entities = vec![runtime::scene_data::EntityData::new(
		EntityKey::new("prop").unwrap(),
		"Prop",
		"props/cube.glb",
	)];
	scene.bindings.clear();
	scene.default_game_camera = None;
	scene.camera.follow = None;
	std::fs::write(
		root.join("assets/scenes/main.json"),
		scene.to_json().unwrap(),
	)
	.unwrap();
	let mut workspace =
		EditorWorkspace::open(root.join("assets").to_str().unwrap(), "scenes/main.json").unwrap();
	workspace
		.backend
		.execute(EditorCommandRequest {
			expected_revision: workspace.backend.session().revision(),
			command: EditorCommandDto::Select {
				keys: vec!["prop".into()],
			},
		})
		.unwrap();
	(Fixture(root), workspace)
}

#[test]
fn prop_preview_preferences_are_token_scoped_without_a_scene_edit() {
	use crate::editor::prop_preview::PropPreviewEnvironment;
	use bridge::viewport::{PropPreviewTargetDto, ViewportShading};

	let (_fixture, mut workspace) = fixture();
	workspace
		.execute_receipt(EditorCommandRequest {
			expected_revision: workspace.backend.session().revision(),
			command: EditorCommandDto::SetSceneEnvironment {
				ambient_linear: [0.2, 0.4, 0.6],
				sky_exposure_ev: 1.5,
			},
		})
		.unwrap();
	let revision = workspace.backend.session().revision();
	let original = workspace
		.backend
		.session()
		.authored_document()
		.to_json()
		.unwrap();
	let saved = std::fs::read(&workspace.scene_path).unwrap();
	let source = workspace
		.create_linked_prefab_source("prefabs/environment.json", BTreeMap::new(), revision)
		.unwrap();
	let target = PropPreviewTargetDto::Source {
		draft_id: source.draft_id,
	};
	let token = workspace
		.open_prop_preview(target.clone(), revision, workspace.generation, 1)
		.unwrap()
		.token;
	assert!(workspace.prop_scene_environment(&token).is_none());
	workspace
		.set_prop_preview_environment(&token, PropPreviewEnvironment::Scene)
		.unwrap();
	assert_eq!(
		workspace
			.prop_scene_environment(&token)
			.unwrap()
			.environment
			.ambient_linear,
		[0.2, 0.4, 0.6]
	);
	workspace
		.set_prop_preview_display(&token, ViewportShading::Wireframe, true, false)
		.unwrap();
	let display = workspace.prop_preview_display(&token).unwrap();
	assert_eq!(display.shading, ViewportShading::Wireframe);
	assert!(display.show_vertices);
	assert!(!display.show_normal_detail);
	assert!(workspace.close_prop_preview(&token));
	let new = workspace
		.open_prop_preview(target, revision, workspace.generation, 2)
		.unwrap();
	assert!(workspace
		.set_prop_preview_environment(&token, PropPreviewEnvironment::Scene)
		.is_err());
	assert!(workspace
		.set_prop_preview_display(&token, ViewportShading::Unlit, true, false)
		.is_err());
	assert!(workspace.prop_scene_environment(&new.token).is_none());
	let display = workspace.prop_preview_display(&new.token).unwrap();
	assert_eq!(display.shading, ViewportShading::Lit);
	assert!(!display.show_vertices);
	assert!(display.show_normal_detail);
	assert_eq!(workspace.backend.session().revision(), revision);
	assert_eq!(
		workspace
			.backend
			.session()
			.authored_document()
			.to_json()
			.unwrap(),
		original
	);
	assert_eq!(std::fs::read(&workspace.scene_path).unwrap(), saved);
}

#[test]
fn reviewed_prop_creation_and_placement_are_separate_and_guarded() {
	let (_fixture, mut workspace) = fixture();
	let revision = workspace.backend.session().revision();
	let original = workspace.backend.session().authored_document();
	let path = "prefabs/assembly.json";
	let output = workspace.asset_root.join(path);

	let invalid = EntityKey::new("outside").unwrap();
	let error = workspace
		.create_linked_prefab_source_at(path, BTreeMap::new(), Some(&invalid), revision)
		.err()
		.unwrap();
	assert!(error.contains("outside"));
	assert!(!output.exists());

	let pivot = EntityKey::new("prop").unwrap();
	let draft = workspace
		.create_linked_prefab_source_at(path, BTreeMap::new(), Some(&pivot), revision)
		.unwrap();
	let published = std::fs::read(&output).unwrap();
	let duplicate = workspace
		.create_linked_prefab_source_at(path, BTreeMap::new(), Some(&pivot), revision)
		.err()
		.unwrap();
	assert!(duplicate.contains("already exists"));
	assert_eq!(std::fs::read(output).unwrap(), published);
	assert_eq!(workspace.backend.session().authored_document(), original);
	assert_eq!(workspace.backend.session().revision(), revision);
	let source = draft.asset_id.clone();
	workspace
		.instantiate_linked_source(
			path,
			&draft.draft_id,
			[2.0, 0.0, 0.0],
			BTreeMap::new(),
			revision,
		)
		.unwrap();
	let revision = workspace.backend.session().revision();
	let second = workspace.open_prefab_source(path, revision).unwrap();
	workspace
		.instantiate_linked_source(
			path,
			&second.draft_id,
			[6.0, 0.0, 0.0],
			BTreeMap::new(),
			revision,
		)
		.unwrap();
	let authored = workspace.backend.session().authored_document();
	let instances: Vec<_> = authored
		.entities
		.iter()
		.filter_map(|entity| {
			entity.linked_prefab.as_ref().map(|link| {
				(
					&entity.key,
					entity.transform.position,
					&link.source.source_id,
				)
			})
		})
		.collect();
	assert_eq!(instances.len(), 2);
	assert_ne!(instances[0].0, instances[1].0);
	assert_eq!(instances[0].1, [2.0, 0.0, 0.0]);
	assert_eq!(instances[1].1, [6.0, 0.0, 0.0]);
	assert_eq!(instances[0].2, &source);
	assert_eq!(instances[1].2, &source);
}
#[test]
fn reviewed_link_rejects_source_changes_before_instance_publication() {
	let (_fixture, mut workspace) = fixture();
	let revision = workspace.backend.session().revision();
	let draft = workspace
		.create_linked_prefab_source("prefabs/prop.json", BTreeMap::new(), revision)
		.unwrap();
	let before = workspace.backend.session().authored_document().clone();
	let mut external = draft.prefab.clone();
	external["entities"][0]["name"] = "Unreviewed replacement".into();
	std::fs::write(
		workspace.asset_root.join("prefabs/prop.json"),
		external.to_string(),
	)
	.unwrap();
	assert!(
		workspace
			.instantiate_linked_source(
				"prefabs/prop.json",
				&draft.draft_id,
				[0.0; 3],
				BTreeMap::new(),
				revision
			)
			.is_err(),
		"a reviewed source must not silently instantiate later disk content"
	);
	assert_eq!(workspace.backend.session().revision(), revision);
	assert_eq!(workspace.backend.session().authored_document(), before);
}
#[test]
fn reviewed_update_rejects_replaced_bytes_and_accepts_a_fresh_review() {
	let (_fixture, mut workspace) = fixture();
	let revision = workspace.backend.session().revision();
	let draft = workspace
		.create_linked_prefab_source("prefabs/prop.json", BTreeMap::new(), revision)
		.unwrap();
	workspace
		.instantiate_linked_source(
			"prefabs/prop.json",
			&draft.draft_id,
			[0.0; 3],
			BTreeMap::new(),
			revision,
		)
		.unwrap();
	workspace.close_prefab_source(&draft.draft_id).unwrap();
	let revision = workspace.backend.session().revision();
	let before = workspace.backend.session().authored_document().clone();
	let root = before
		.entities
		.iter()
		.find(|entity| entity.linked_prefab.is_some())
		.unwrap()
		.key
		.clone();
	let reviewed = workspace
		.open_prefab_source("prefabs/prop.json", revision)
		.unwrap();
	let mut external = reviewed.prefab.clone();
	external["entities"][0]["name"] = "Changed after review".into();
	std::fs::write(
		workspace.asset_root.join("prefabs/prop.json"),
		external.to_string(),
	)
	.unwrap();
	let error = workspace
		.update_linked_source(
			root.as_str(),
			&reviewed.draft_id,
			PrefabConflictResolution::UseSource,
			None,
			revision,
		)
		.err()
		.unwrap();
	assert!(error.contains("changed on disk"), "{error}");
	assert_eq!(workspace.backend.session().revision(), revision);
	assert_eq!(workspace.backend.session().authored_document(), before);
	workspace.close_prefab_source(&reviewed.draft_id).unwrap();
	let fresh = workspace
		.open_prefab_source("prefabs/prop.json", revision)
		.unwrap();
	workspace
		.update_linked_source(
			root.as_str(),
			&fresh.draft_id,
			PrefabConflictResolution::UseSource,
			None,
			revision,
		)
		.unwrap();
	assert!(workspace
		.backend
		.session()
		.authored_document()
		.entities
		.iter()
		.any(|entity| entity.name == "Changed after review"));
	workspace
		.backend
		.execute(EditorCommandRequest {
			expected_revision: workspace.backend.session().revision(),
			command: EditorCommandDto::Undo,
		})
		.unwrap();
	assert_eq!(workspace.backend.session().authored_document(), before);
}
#[test]
fn reviewed_link_rejects_foreign_expired_and_replaced_identity_captures() {
	let (_fixture, mut workspace) = fixture();
	let revision = workspace.backend.session().revision();
	let draft = workspace
		.create_linked_prefab_source("prefabs/prop.json", BTreeMap::new(), revision)
		.unwrap();
	let foreign = workspace
		.create_linked_prefab_source("prefabs/other.json", BTreeMap::new(), revision)
		.unwrap();
	let before = workspace.backend.session().authored_document().clone();
	assert!(workspace
		.instantiate_linked_source(
			"prefabs/prop.json",
			&foreign.draft_id,
			[0.0; 3],
			BTreeMap::new(),
			revision
		)
		.err()
		.unwrap()
		.contains("differs"));
	workspace.close_prefab_source(&foreign.draft_id).unwrap();
	assert!(workspace
		.instantiate_linked_source(
			"prefabs/other.json",
			&foreign.draft_id,
			[0.0; 3],
			BTreeMap::new(),
			revision
		)
		.err()
		.unwrap()
		.contains("expired"));
	let catalog_path = workspace
		.asset_root
		.join(runtime::asset_catalog::CATALOG_FILE);
	let mut catalog = runtime::assets::project_assets::load_catalog(&workspace.asset_root).unwrap();
	catalog
		.assets
		.iter_mut()
		.find(|asset| asset.path == "prefabs/prop.json")
		.unwrap()
		.id = AssetId::generate();
	std::fs::write(catalog_path, catalog.to_json().unwrap()).unwrap();
	assert!(workspace
		.instantiate_linked_source(
			"prefabs/prop.json",
			&draft.draft_id,
			[0.0; 3],
			BTreeMap::new(),
			revision
		)
		.err()
		.unwrap()
		.contains("differs"));
	assert_eq!(workspace.backend.session().revision(), revision);
	assert_eq!(workspace.backend.session().authored_document(), before);
}
#[test]
fn generic_host_commands_cannot_bypass_reviewed_source_authority() {
	let (_fixture, mut workspace) = fixture();
	let revision = workspace.backend.session().revision();
	let draft = workspace
		.create_linked_prefab_source("prefabs/prop.json", BTreeMap::new(), revision)
		.unwrap();
	let source = workspace.prefab_source("prefabs/prop.json").unwrap();
	let error = workspace
		.execute_receipt(EditorCommandRequest {
			expected_revision: revision,
			command: EditorCommandDto::InstantiateLinkedPrefab {
				source: source.clone(),
				position: [0.0; 3],
				bindings: BTreeMap::new(),
			},
		})
		.unwrap_err();
	assert!(error.message.contains("guarded prefab"));
	assert_eq!(workspace.backend.session().revision(), revision);
	workspace
		.instantiate_linked_source(
			"prefabs/prop.json",
			&draft.draft_id,
			[0.0; 3],
			BTreeMap::new(),
			revision,
		)
		.unwrap();
	let revision = workspace.backend.session().revision();
	let before = workspace.backend.session().authored_document().clone();
	let root = before
		.entities
		.iter()
		.find(|entity| entity.linked_prefab.is_some())
		.unwrap()
		.key
		.as_str()
		.to_owned();
	let error = workspace
		.execute_receipt(EditorCommandRequest {
			expected_revision: revision,
			command: EditorCommandDto::UpdateLinkedPrefab {
				root_key: root,
				source,
				resolution: PrefabConflictResolution::UseSource,
				bindings: None,
			},
		})
		.unwrap_err();
	assert!(error.message.contains("guarded prefab"));
	assert_eq!(workspace.backend.session().revision(), revision);
	assert_eq!(workspace.backend.session().authored_document(), before);
}
#[test]
fn source_draft_saves_canonical_components_and_conflicts_with_external_disk_edits() {
	let (_fixture, mut workspace) = fixture();
	let revision = workspace.backend.session().revision();
	let draft = workspace
		.create_linked_prefab_source("prefabs/prop.json", BTreeMap::new(), revision)
		.unwrap();
	assert!(draft.prefab["entities"][0].get("components").is_some());
	let mut value = draft.prefab.clone();
	value["entities"][0]["name"] = "Edited".into();
	workspace
		.validate_prefab_source(&draft.draft_id, value.clone(), revision)
		.unwrap();
	let saved = workspace
		.save_prefab_source(&draft.draft_id, value.clone(), revision)
		.unwrap();
	assert_ne!(saved.draft.fingerprint, draft.fingerprint);
	assert_eq!(
		workspace.prop_source_draft_revisions(),
		vec![(saved.draft.draft_id.clone(), saved.draft.draft_revision)]
	);
	let path = workspace.asset_root.join("prefabs/prop.json");
	let catalog = std::fs::read(
		workspace
			.asset_root
			.join(runtime::asset_catalog::CATALOG_FILE),
	)
	.unwrap();
	value["entities"][0]["name"] = "Extern".into();
	let external = serde_json::to_vec(&value).unwrap();
	std::fs::write(&path, &external).unwrap();
	assert!(workspace
		.save_prefab_source(&draft.draft_id, saved.draft.prefab.clone(), revision)
		.err()
		.unwrap()
		.contains("changed on disk"));
	assert_eq!(std::fs::read(path).unwrap(), external);
	assert_eq!(
		std::fs::read(
			workspace
				.asset_root
				.join(runtime::asset_catalog::CATALOG_FILE)
		)
		.unwrap(),
		catalog
	);
	workspace.close_prefab_source(&draft.draft_id).unwrap();
	assert!(workspace
		.validate_prefab_source(&draft.draft_id, value, revision)
		.is_err());
}
#[test]
fn source_updates_are_explicit_undoable_and_missing_sources_do_not_break_pinned_play() {
	let (_fixture, mut workspace) = fixture();
	let revision = workspace.backend.session().revision();
	let draft = workspace
		.create_linked_prefab_source("prefabs/prop.json", BTreeMap::new(), revision)
		.unwrap();
	workspace
		.instantiate_linked_source(
			"prefabs/prop.json",
			&draft.draft_id,
			[2.0, 0.0, 0.0],
			BTreeMap::new(),
			revision,
		)
		.unwrap();
	let revision = workspace.backend.session().revision();
	let scene = workspace.backend.session().authored_document();
	let anchor = scene
		.entities
		.iter()
		.find(|entity| entity.linked_prefab.is_some())
		.unwrap();
	let root = anchor.key.clone();
	let member = anchor
		.linked_prefab
		.as_ref()
		.unwrap()
		.entity_map
		.values()
		.next()
		.unwrap()
		.clone();
	drop(scene);
	let opened = workspace
		.open_prefab_source("prefabs/prop.json", revision)
		.unwrap();
	let mut value = opened.prefab;
	value["entities"][0]["name"] = "New source name".into();
	workspace
		.save_prefab_source(&opened.draft_id, value, revision)
		.unwrap();
	assert_eq!(
		workspace
			.backend
			.session()
			.authored_document()
			.entities
			.iter()
			.find(|entity| entity.key == member)
			.unwrap()
			.name,
		"Prop"
	);
	assert!(
		workspace
			.linked_prefab_status(member.as_str(), revision)
			.unwrap()
			.unwrap()
			.source_changed
	);
	workspace
		.update_linked_source(
			root.as_str(),
			&opened.draft_id,
			PrefabConflictResolution::Reject,
			None,
			revision,
		)
		.unwrap();
	assert_eq!(
		workspace
			.backend
			.session()
			.authored_document()
			.entities
			.iter()
			.find(|entity| entity.key == member)
			.unwrap()
			.name,
		"New source name"
	);
	workspace
		.backend
		.execute(EditorCommandRequest {
			expected_revision: workspace.backend.session().revision(),
			command: EditorCommandDto::Undo,
		})
		.unwrap();
	assert_eq!(
		workspace
			.backend
			.session()
			.authored_document()
			.entities
			.iter()
			.find(|entity| entity.key == member)
			.unwrap()
			.name,
		"Prop"
	);
	std::fs::remove_file(workspace.asset_root.join("prefabs/prop.json")).unwrap();
	let revision = workspace.backend.session().revision();
	assert!(workspace
		.linked_prefab_status(root.as_str(), revision)
		.unwrap()
		.unwrap()
		.source_error
		.is_some());
	let before = workspace.backend.snapshot().unwrap();
	assert!(workspace
		.update_linked_source(
			root.as_str(),
			&opened.draft_id,
			PrefabConflictResolution::Reject,
			None,
			revision
		)
		.is_err());
	assert_eq!(workspace.backend.session().revision(), before.revision);
	workspace
		.backend
		.execute(EditorCommandRequest {
			expected_revision: revision,
			command: EditorCommandDto::StartPlay,
		})
		.unwrap();
	workspace
		.backend
		.execute(EditorCommandRequest {
			expected_revision: workspace.backend.session().revision(),
			command: EditorCommandDto::StopPlay,
		})
		.unwrap();
	assert!(workspace
		.backend
		.session()
		.authored_document()
		.entities
		.iter()
		.any(|entity| entity.linked_prefab.is_some()));
	assert!(workspace
		.validate_prefab_source(
			&draft.draft_id,
			draft.prefab,
			workspace.backend.session().revision()
		)
		.is_err());
}
#[test]
fn drafts_reject_other_generation_and_component_sources_cannot_overwrite_scenes() {
	let (_fixture, mut workspace) = fixture();
	let revision = workspace.backend.session().revision();
	assert!(workspace
		.create_linked_prefab_source("scenes/main.json", BTreeMap::new(), revision)
		.is_err());
	let draft = workspace
		.create_linked_prefab_source("prefabs/prop.json", BTreeMap::new(), revision)
		.unwrap();
	workspace.generation += 1;
	assert!(workspace
		.save_prefab_source(&draft.draft_id, draft.prefab, revision)
		.err()
		.unwrap()
		.contains("generation"));
}
#[test]
fn component_source_creation_declares_external_actor_slot_and_link_binds_it_explicitly() {
	use runtime::{
		components::{ComponentId, NativeBehaviorComponent, NativeBehaviorSpec},
		scene_data::{ColliderData, ColliderInteraction},
	};
	let (_fixture, mut workspace) = fixture();
	let mut scene = (*workspace.backend.session().authored_document()).clone();
	let actor = EntityKey::new("actor").unwrap();
	let door = EntityKey::new("door").unwrap();
	let trigger = EntityKey::new("trigger").unwrap();
	let solid = ColliderData::Sphere {
		radius: 0.2,
		offset: [0.0; 3],
		interaction: ColliderInteraction::default(),
	};
	let sensor = ColliderData::Sphere {
		radius: 0.2,
		offset: [0.0; 3],
		interaction: ColliderInteraction {
			sensor: true,
			..Default::default()
		},
	};
	let mut door_entity =
		runtime::scene_data::EntityData::new(door.clone(), "Door", "props/cube.glb")
			.with_collider(Some(solid.clone()));
	door_entity.native_behaviors.push(NativeBehaviorComponent {
		id: ComponentId::new("key-door").unwrap(),
		enabled: true,
		behavior: NativeBehaviorSpec::KeyDoor {
			actor: actor.clone(),
			key_trigger: trigger.clone(),
			door: door.clone(),
			key_visual: None,
		},
	});
	scene.entities = vec![
		runtime::scene_data::EntityData::new(actor.clone(), "Actor", "props/cube.glb")
			.with_collider(Some(solid)),
		door_entity,
		runtime::scene_data::EntityData::new(trigger.clone(), "Trigger", "props/cube.glb")
			.with_collider(Some(sensor)),
	];
	let project =
		runtime::project::ProjectData::for_scene("Slots", "scenes/main.json", &scene).unwrap();
	workspace.backend = runtime::Engine::new(scene, project).unwrap();
	workspace
		.backend
		.execute(EditorCommandRequest {
			expected_revision: 0,
			command: EditorCommandDto::Select {
				keys: vec![door.as_str().into(), trigger.as_str().into()],
			},
		})
		.unwrap();
	let revision = workspace.backend.session().revision();
	assert!(workspace
		.create_linked_prefab_source("prefabs/unbound.json", BTreeMap::new(), revision)
		.is_err());
	let draft = workspace
		.create_linked_prefab_source(
			"prefabs/door.json",
			BTreeMap::from([("actor".into(), actor.clone())]),
			revision,
		)
		.unwrap();
	assert_eq!(draft.prefab["reference_slots"]["actor"], actor.as_str());
	assert!(workspace
		.instantiate_linked_source(
			"prefabs/door.json",
			&draft.draft_id,
			[1.0, 0.0, 0.0],
			BTreeMap::new(),
			revision
		)
		.is_err());
	workspace
		.instantiate_linked_source(
			"prefabs/door.json",
			&draft.draft_id,
			[1.0, 0.0, 0.0],
			BTreeMap::from([("actor".into(), actor)]),
			revision,
		)
		.unwrap();
	workspace
		.backend
		.session()
		.authored_document()
		.validate()
		.unwrap();
}
