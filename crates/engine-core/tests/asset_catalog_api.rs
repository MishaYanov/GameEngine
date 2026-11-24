use engine_core::asset_catalog::{
	AssetCatalog, AssetId, AssetKind, MeshIdentity, MeshSignature, MeshSubassetId, PersistentAsset,
};
use std::collections::HashSet;
mod support;

fn model(path: &str) -> PersistentAsset {
	PersistentAsset {
		id: AssetId::generate(),
		path: path.into(),
		kind: AssetKind::Model,
		aliases: vec![],
		revision: 1,
		fingerprint: "a".repeat(64),
		bytes: 128,
		source_path: None,
		meshes: vec![MeshSignature {
			index: 0,
			name: "Cube".into(),
			primitives: 1,
			identity: None,
		}],
		dependencies: vec![],
	}
}

#[test]
fn stable_identity_and_reserved_legacy_alias_survive_move_and_json_roundtrip() {
	let mut catalog = AssetCatalog::default();
	let mut asset = model("props/cube.glb");
	let id = asset.id.clone();
	catalog.replace(asset.clone()).unwrap();
	asset.aliases.push(asset.path.clone());
	asset.path = "moved/renamed.glb".into();
	asset.revision += 1;
	catalog.replace(asset).unwrap();
	let reopened = AssetCatalog::from_json(&catalog.to_json().unwrap()).unwrap();
	for reference in [Some(&id), None] {
		assert_eq!(
			reopened
				.resolve(reference, "props/cube.glb", None)
				.unwrap()
				.unwrap()
				.path,
			"moved/renamed.glb"
		);
	}
	let before = catalog.clone();
	assert!(catalog.replace(model("props/cube.glb")).is_err());
	assert_eq!(catalog, before);
	assert!(
		catalog
			.resolve(Some(&AssetId::generate()), "moved/renamed.glb", None)
			.is_err()
	);
}

#[test]
fn stable_mesh_references_round_trip_through_scenes_and_props() {
	use engine_core::{SceneData, editor::PrefabData, scene_data::EntityData};

	let model_id = AssetId::generate();
	let stable_id = MeshSubassetId::generate();
	let entity = EntityData::new(support::key("arch"), "Arch", "props/arch.glb")
		.with_asset_id(Some(model_id.clone()))
		.with_mesh_index(Some(5))
		.with_mesh_id(Some(stable_id.clone()));
	let entity_expected = (Some(model_id), Some(stable_id), Some(5));
	let check = |entity: &EntityData| {
		let reference = entity.model_reference().unwrap();
		assert_eq!(
			(reference.asset_id, reference.mesh_id, reference.mesh_index),
			entity_expected
		);
	};
	let scene = support::scene(vec![entity.clone()]);
	let scene_json = scene.to_json().unwrap();
	assert!(scene_json.contains("mesh_id"));
	check(&SceneData::from_json(&scene_json).unwrap().entities[0]);
	let prefab = PrefabData {
		schema_version: 3,
		composition: Default::default(),
		entities: vec![entity],
		gameplay: None,
		reference_slots: Default::default(),
	};
	let prefab_json = prefab.to_json().unwrap();
	check(&PrefabData::from_json(&prefab_json).unwrap().entities[0]);

	let legacy_entity = support::entity("legacy", [0.; 3]).with_mesh_index(Some(3));
	let legacy_json = support::scene(vec![legacy_entity]).to_json().unwrap();
	assert!(!legacy_json.contains("mesh_id"));
	let legacy = SceneData::from_json(&legacy_json).unwrap();
	assert_eq!(legacy.entities[0].mesh_index, Some(3));
	assert!(legacy.entities[0].mesh_id.is_none());
}

#[test]
fn reimport_requires_referenced_mesh_compatibility_without_changing_the_original() {
	let original = model("props/cube.glb");
	let used = HashSet::from([0]);
	let mut next = original.clone();
	next.revision += 1;
	next.fingerprint = "b".repeat(64);
	original.check_reimport(&next, &used).unwrap();
	next.meshes[0].name = "Other mesh".into();
	assert!(original.check_reimport(&next, &used).is_err());
	next.meshes.clear();
	assert!(original.check_reimport(&next, &used).is_err());
	assert_eq!(original.meshes[0].name, "Cube");
}

#[test]
fn stable_mesh_ids_validate_uuid_digests_and_unique_source_keys() {
	let mut asset = model("mesh.glb");
	assert!(asset.meshes[0].identity.is_none());
	let id = MeshSubassetId::generate();
	asset.meshes[0].identity = Some(MeshIdentity {
		id: id.clone(),
		source_key: Some("hull-01".into()),
		geometry_sha256: "d".repeat(64),
	});
	let mut catalog = AssetCatalog::default();
	catalog.replace(asset.clone()).unwrap();
	let encoded = catalog.to_json().unwrap();
	let reopened = AssetCatalog::from_json(&encoded).unwrap();
	assert_eq!(
		reopened
			.by_id(&asset.id)
			.unwrap()
			.mesh_by_id(&id)
			.unwrap()
			.index,
		0
	);
	assert!(AssetCatalog::from_json(&encoded.replace(id.as_str(), "invalid")).is_err());

	let mut repeated = asset.meshes[0].clone();
	repeated.index = 1;
	asset.meshes.push(repeated);
	assert!(asset.validate().is_err());
	asset.meshes[1].identity.as_mut().unwrap().id = MeshSubassetId::generate();
	assert!(asset.validate().is_err());
	asset.meshes[1].identity.as_mut().unwrap().source_key = Some("bad/key".into());
	assert!(asset.validate().is_err());
}

#[test]
fn catalog_rejects_bad_identity_paths_duplicate_aliases_and_unresolved_dependencies() {
	for id in [
		"",
		"00000000-0000-0000-0000-000000000000",
		"../props/cube.glb",
	] {
		assert!(AssetId::new(id).is_err());
	}
	let mut catalog = AssetCatalog::default();
	let mut asset = model("props/cube.glb");
	asset.aliases.push("../outside.glb".into());
	assert!(catalog.replace(asset.clone()).is_err());
	asset.aliases = vec!["PROPS/CUBE.GLB".into()];
	assert!(catalog.replace(asset.clone()).is_err());
	asset.aliases.clear();
	asset.dependencies.push(AssetId::generate());
	assert!(catalog.replace(asset).is_err());
	for source in [
		"[]",
		r#"{"schema_version":1,"assets":[[]]}"#,
		r#"{"schema_version":2,"assets":[]}"#,
		r#"{"schema_version":1,"assets":[],"extra":true}"#,
	] {
		assert!(AssetCatalog::from_json(source).is_err());
	}
}

#[test]
fn asset_invalidation_refreshes_rendering_and_stale_commands_without_dirtying_or_losing_history() {
	use engine_core::{
		Engine,
		command::{EditorCommandDto as Command, EditorCommandRequest},
		project::ProjectData,
	};
	let scene = support::scene(vec![support::entity("actor", [0.; 3])]);
	let project = ProjectData::for_scene("Assets", "scenes/main.json", &scene).unwrap();
	let mut engine = Engine::new(scene, project).unwrap();
	engine
		.execute(EditorCommandRequest {
			expected_revision: 0,
			command: Command::Rename {
				key: "actor".into(),
				name: "Edited".into(),
			},
		})
		.unwrap();
	let before = engine.snapshot().unwrap();
	let document = engine.session().authored_document();
	let project = engine.session().project().clone();
	let old_revision = engine.session().revision();
	let cursor = engine.change_cursor();
	let receipt = engine.invalidate_assets().unwrap();
	assert!(receipt.changes.full_resync);
	assert!(!receipt.project_changed);
	assert_eq!(receipt.revision, old_revision + 1);
	assert_eq!(engine.session().authored_document(), document);
	assert_eq!(engine.session().project(), &project);
	let after = engine.snapshot().unwrap();
	assert_eq!(after.dirty, before.dirty);
	assert_eq!(after.project_dirty, before.project_dirty);
	assert!(after.can_undo);
	assert!(engine.change_cursor() > cursor);
	assert!(
		engine
			.execute(EditorCommandRequest {
				expected_revision: old_revision,
				command: Command::Undo
			})
			.is_err()
	);
	engine
		.execute(EditorCommandRequest {
			expected_revision: receipt.revision,
			command: Command::Undo,
		})
		.unwrap();
	assert_eq!(
		engine.session().authored_document().entities[0].name,
		"actor"
	);
	for command in [
		Command::BeginTransaction {
			label: "Edit".into(),
		},
		Command::CancelTransaction,
		Command::StartPlay,
		Command::StopPlay,
	] {
		engine
			.execute(EditorCommandRequest {
				expected_revision: engine.session().revision(),
				command,
			})
			.unwrap();
		if engine.session().has_transaction() || engine.session().is_playing() {
			let revision = engine.session().revision();
			let cursor = engine.change_cursor();
			assert!(engine.invalidate_assets().is_err());
			assert_eq!(engine.session().revision(), revision);
			assert_eq!(engine.change_cursor(), cursor);
		}
	}
}
