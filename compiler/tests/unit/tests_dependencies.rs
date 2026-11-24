use super::*;
use crate::test_project;

use engine_core::{
	EntityKey,
	asset_catalog::{
		AssetCatalog, AssetId, AssetKind, CATALOG_FILE, MeshSignature, PersistentAsset,
	},
	editor::PrefabData,
	scene_data::EntityData,
};

fn model_record(source: &Path, path: &str, bundled: &str) -> PersistentAsset {
	let target = source.join(path);
	fs::create_dir_all(target.parent().unwrap()).unwrap();
	fs::copy(
		Path::new(env!("CARGO_MANIFEST_DIR"))
			.join("../apps/game/assets/props")
			.join(bundled),
		&target,
	)
	.unwrap();
	let bytes = fs::read(target).unwrap();
	let model = gltf::Gltf::from_slice(&bytes).unwrap();
	PersistentAsset {
		id: AssetId::generate(),
		path: path.into(),
		kind: AssetKind::Model,
		aliases: Vec::new(),
		revision: 1,
		fingerprint: format!("{:x}", Sha256::digest(&bytes)),
		bytes: bytes.len() as u64,
		source_path: Some("C:/private/artist/source.glb".into()),
		meshes: model
			.meshes()
			.map(|mesh| MeshSignature {
				index: mesh.index(),
				name: mesh.name().unwrap_or("Mesh").into(),
				primitives: mesh.primitives().len(),
				identity: None,
			})
			.collect(),
		dependencies: Vec::new(),
	}
}
fn save_catalog(source: &Path, assets: Vec<PersistentAsset>) {
	fs::write(
		source.join(CATALOG_FILE),
		AssetCatalog {
			schema_version: 1,
			assets,
		}
		.to_json()
		.unwrap(),
	)
	.unwrap();
}

fn spawner(entity: &str, path: &str) -> EntityData {
	use engine_core::components::{
		ComponentId, NO_MESH_PATH, NativeBehaviorComponent, NativeBehaviorSpec,
	};
	let mut entity = EntityData::new(EntityKey::new(entity).unwrap(), "Spawner", NO_MESH_PATH)
		.with_renderable(false);
	entity.native_behaviors.push(NativeBehaviorComponent {
		id: ComponentId::new("spawner").unwrap(),
		enabled: true,
		behavior: NativeBehaviorSpec::TimedSpawner {
			prefab_path: path.into(),
			interval_ticks: 1,
			max_alive: 1,
			lifetime_ticks: 2,
			spawn_offset: [0.0; 3],
			bindings: Default::default(),
		},
	});
	entity
}

mod model_dependencies {
	use super::*;

	#[test]
	fn reserved_root_catalog_is_not_a_model_payload_dependency() {
		for uri in ["../asset-catalog.json", "../%61sset-catalog.json"] {
			let fixture = AssetFixture::new();
			let source = fixture.0.join("source");
			let (project, _) = authored_scene_fixture(&source);
			save_catalog(&source, vec![]);
			attach_payload_dependency(&source, uri, 1);
			let error = prepare_project(&project, &source).err().unwrap();
			assert!(
				error.contains("reserved root asset-catalog.json"),
				"{error}"
			);
			assert!(!fixture.0.join(&project.name).exists());
		}
	}

	#[cfg(windows)]
	#[test]
	fn case_variant_references_stage_one_canonical_model_and_dependency_snapshot() {
		let fixture = AssetFixture::new();
		let source = fixture.0.join("source");
		let (project, _) = authored_scene_fixture(&source);
		fs::write(source.join("payload.bin"), [0u8; 36]).unwrap();
		attach_payload_dependency(&source, "../PAYLOAD.BIN", 36);
		let mut scene = read_scene_file(&source.join("scenes/main.json")).unwrap();
		scene.entities[0].model_path = "MODELS/PAYLOAD.GLTF".into();
		fs::write(source.join("scenes/main.json"), scene.to_json().unwrap()).unwrap();
		fs::create_dir(source.join("textures")).unwrap();
		let image = fs::read(
			Path::new(env!("CARGO_MANIFEST_DIR")).join("../apps/game/assets/textures/crate.png"),
		)
		.unwrap();
		fs::write(source.join("textures/crate.png"), &image).unwrap();
		let path = source.join("models/payload.gltf");
		let mut model: serde_json::Value =
			serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
		model["buffers"]
			.as_array_mut()
			.unwrap()
			.push(serde_json::json!({"uri":"../payload.bin","byteLength":36}));
		model["images"] = serde_json::json!([{"uri":"../TEXTURES/CRATE.PNG"}]);
		fs::write(path, serde_json::to_vec(&model).unwrap()).unwrap();
		let texture = PersistentAsset {
			id: AssetId::generate(),
			path: "TEXTURES/CRATE.PNG".into(),
			kind: AssetKind::Texture,
			aliases: vec![],
			revision: 1,
			fingerprint: format!("{:x}", Sha256::digest(&image)),
			bytes: image.len() as u64,
			source_path: None,
			meshes: vec![],
			dependencies: vec![],
		};
		save_catalog(&source, vec![texture.clone()]);
		let prepared = prepare_project(&project, &source).unwrap();
		assert_eq!(prepared.asset_count(), 5);
		assert!(
			prepared
				.files
				.iter()
				.any(|file| file.relative == Path::new("models/payload.gltf"))
		);
		assert!(
			prepared
				.files
				.iter()
				.any(|file| file.relative == Path::new("payload.bin"))
		);
		let staged = stage_prepared_project(&prepared, fixture.0.to_str().unwrap()).unwrap();
		validate_export_snapshot(&staged, None).unwrap();
		let exported = staged.path().join("assets");
		assert_eq!(fs::read(exported.join("payload.bin")).unwrap(), [0u8; 36]);
		assert!(exported.join("models/payload.gltf").is_file());
		assert_eq!(
			dependencies::read_catalog(&exported).unwrap().assets,
			vec![texture]
		);
	}

	#[test]
	fn dependency_export_resolves_ids_and_legacy_aliases_includes_prefab_models_and_strips_source_metadata()
	 {
		let fixture = AssetFixture::new();
		let source = fixture.0.join("source");
		let (project, mut scene) = authored_scene_fixture(&source);
		let mut main = model_record(&source, "models/moved.glb", "cube.glb");
		main.aliases.push("models/original.glb".into());
		let prefab_model = model_record(&source, "models/prefab-only.glb", "sphere.glb");
		let unused = model_record(&source, "models/unused.glb", "capsule.glb");
		scene.entities.push(
			EntityData::new(
				EntityKey::new("explicit").unwrap(),
				"Explicit ID",
				"models/original.glb",
			)
			.with_asset_id(Some(main.id.clone())),
		);
		scene.entities.push(EntityData::new(
			EntityKey::new("legacy").unwrap(),
			"Legacy alias",
			"models/original.glb",
		));
		fs::write(source.join("scenes/main.json"), scene.to_json().unwrap()).unwrap();
		fs::create_dir(source.join("prefabs")).unwrap();
		let prefab = PrefabData {
			composition: Default::default(),
			reference_slots: Default::default(),
			schema_version: 1,
			entities: vec![
				EntityData::new(
					EntityKey::new("prefab-prop").unwrap(),
					"Prefab prop",
					&prefab_model.path,
				)
				.with_asset_id(Some(prefab_model.id.clone())),
			],
			gameplay: None,
		};
		fs::write(source.join("prefabs/prop.json"), prefab.to_json().unwrap()).unwrap();
		fs::write(source.join("unrelated.blend"), b"not a runtime dependency").unwrap();
		save_catalog(
			&source,
			vec![main.clone(), prefab_model.clone(), unused.clone()],
		);
		let prepared = prepare_project(&project, &source).unwrap();
		let staged = stage_prepared_project(&prepared, fixture.0.to_str().unwrap()).unwrap();
		let exported = staged.path().join("assets");
		assert!(exported.join(&main.path).is_file());
		assert!(exported.join(&prefab_model.path).is_file());
		assert!(exported.join("prefabs/prop.json").is_file());
		assert!(!exported.join(&unused.path).exists());
		assert!(!exported.join("unrelated.blend").exists());
		assert!(!exported.join("models/original.glb").exists());
		let catalog = dependencies::read_catalog(&exported).unwrap();
		assert_eq!(catalog.assets.len(), 2);
		assert!(
			catalog
				.assets
				.iter()
				.all(|asset| asset.source_path.is_none())
		);
		assert_eq!(
			catalog
				.resolve(None, "models/original.glb", None)
				.unwrap()
				.unwrap()
				.id,
			main.id
		);
		assert_eq!(
			read_scene_file(&exported.join("scenes/main.json")).unwrap(),
			scene
		);
		// Verification is entirely against the frozen output, even after the source is moved.
		fs::rename(&source, fixture.0.join("source-retired")).unwrap();
		validate_export_snapshot(&staged, None).unwrap();
		validate_catalog_scenes(&project, &exported.canonicalize().unwrap(), &staged.files)
			.unwrap();
	}

	#[test]
	fn gltf_buffer_and_image_uri_closure_is_complete_and_unused_files_are_omitted() {
		let fixture = AssetFixture::new();
		let source = fixture.0.join("source");
		let (project, _) = authored_scene_fixture(&source);
		fs::create_dir(source.join("textures")).unwrap();
		fs::write(source.join("payload.bin"), b"original").unwrap();
		fs::write(
			source.join("textures/a space.png"),
			b"image dependency bytes",
		)
		.unwrap();
		fs::write(source.join("textures/unused.png"), b"omit").unwrap();
		attach_payload_dependency(&source, "../payload.bin", 8);
		let path = source.join("models/payload.gltf");
		let mut model: serde_json::Value =
			serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
		model["images"] = serde_json::json!([{"uri":"../textures/a%20space.png"}]);
		fs::write(&path, serde_json::to_vec(&model).unwrap()).unwrap();
		let prepared = prepare_project(&project, &source).unwrap();
		let staged = stage_prepared_project(&prepared, fixture.0.to_str().unwrap()).unwrap();
		assert!(staged.path().join("assets/payload.bin").is_file());
		assert!(staged.path().join("assets/textures/a space.png").is_file());
		assert!(!staged.path().join("assets/textures/unused.png").exists());
		fs::remove_file(source.join("textures/a space.png")).unwrap();
		assert!(
			prepare_project(&project, &source)
				.err()
				.unwrap()
				.contains("Dependency")
		);
	}

	#[test]
	fn remote_and_escaping_model_dependencies_fail_before_output_creation() {
		for uri in [
			"../../outside.bin",
			"%2e%2e/%2e%2e/outside.bin",
			"https://example.invalid/asset.bin",
			"C:/outside.bin",
		] {
			let fixture = AssetFixture::new();
			let source = fixture.0.join("source");
			let (project, _) = authored_scene_fixture(&source);
			attach_payload_dependency(&source, uri, 8);
			assert!(prepare_project(&project, &source).is_err(), "{uri}");
			assert!(!fixture.0.join(&project.name).exists());
		}
	}

	#[test]
	fn changed_catalog_and_unknown_asset_identity_fail_without_a_partial_output() {
		let fixture = AssetFixture::new();
		let source = fixture.0.join("source");
		let (project, mut scene) = authored_scene_fixture(&source);
		let main = model_record(&source, "models/main.glb", "cube.glb");
		scene.entities.push(
			EntityData::new(EntityKey::new("actor").unwrap(), "Actor", &main.path)
				.with_asset_id(Some(main.id.clone())),
		);
		fs::write(source.join("scenes/main.json"), scene.to_json().unwrap()).unwrap();
		save_catalog(&source, vec![main.clone()]);
		let prepared = prepare_project(&project, &source).unwrap();
		let mut changed = main;
		changed.source_path = Some("C:/different/source.glb".into());
		save_catalog(&source, vec![changed]);
		let error = stage_prepared_project(&prepared, fixture.0.to_str().unwrap())
			.err()
			.unwrap();
		assert!(error.contains("catalog changed"), "{error}");
		assert!(!fixture.0.join(&project.name).exists());
		scene.entities[0].asset_id = Some(AssetId::generate());
		fs::write(source.join("scenes/main.json"), scene.to_json().unwrap()).unwrap();
		assert!(
			prepare_project(&project, &source)
				.err()
				.unwrap()
				.contains("unavailable")
		);
	}

	#[test]
	fn long_catalog_dependency_chains_fail_with_a_bounded_depth_error() {
		let fixture = AssetFixture::new();
		let source = fixture.0.join("source");
		let (project, mut scene) = authored_scene_fixture(&source);
		let mut main = model_record(&source, "models/main.glb", "cube.glb");
		let ids = (0..70).map(|_| AssetId::generate()).collect::<Vec<_>>();
		main.dependencies.push(ids[0].clone());
		scene.entities.push(
			EntityData::new(EntityKey::new("actor").unwrap(), "Actor", &main.path)
				.with_asset_id(Some(main.id.clone())),
		);
		fs::write(source.join("scenes/main.json"), scene.to_json().unwrap()).unwrap();
		let mut assets = vec![main];
		for (index, id) in ids.iter().enumerate() {
			let path = format!("texture-{index}.png");
			fs::write(source.join(&path), [0u8]).unwrap();
			assets.push(PersistentAsset {
				id: id.clone(),
				path,
				kind: AssetKind::Texture,
				aliases: Vec::new(),
				revision: 1,
				fingerprint: format!("{:x}", Sha256::digest([0u8])),
				bytes: 1,
				source_path: None,
				meshes: Vec::new(),
				dependencies: ids.get(index + 1).cloned().into_iter().collect(),
			});
		}
		save_catalog(&source, assets);
		assert!(
			prepare_project(&project, &source)
				.err()
				.unwrap()
				.contains("64 levels")
		);
		assert!(!fixture.0.join(&project.name).exists());
	}
}

mod prefab_dependencies {
	use super::*;

	#[test]
	fn linked_export_resolves_pinned_entities_without_reading_missing_or_changed_source() {
		use engine_core::{editor::EditorSession, linked_prefab::LinkedPrefabSource};
		let fixture = AssetFixture::new();
		let source = fixture.0.join("source");
		let (project, scene) = authored_scene_fixture(&source);
		let model = model_record(&source, "models/cube.glb", "cube.glb");
		let prefab = PrefabData {
			schema_version: 3,
			composition: Default::default(),
			entities: vec![
				EntityData::new(
					EntityKey::new("prop").unwrap(),
					"Pinned prop",
					"models/cube.glb",
				)
				.with_asset_id(Some(model.id.clone())),
			],
			gameplay: None,
			reference_slots: Default::default(),
		};
		fs::create_dir(source.join("prefabs")).unwrap();
		let bytes = prefab.to_json().unwrap().into_bytes();
		fs::write(source.join("prefabs/source.json"), &bytes).unwrap();
		let record = PersistentAsset {
			id: AssetId::generate(),
			path: "prefabs/source.json".into(),
			kind: AssetKind::Prefab,
			aliases: vec![],
			revision: 1,
			fingerprint: format!("{:x}", Sha256::digest(&bytes)),
			bytes: bytes.len() as u64,
			source_path: None,
			meshes: vec![],
			dependencies: vec![model.id.clone()],
		};
		save_catalog(&source, vec![model, record.clone()]);
		let mut session = EditorSession::for_project(scene, project.clone()).unwrap();
		session
			.instantiate_linked_prefab(
				LinkedPrefabSource {
					source_id: record.id,
					path: record.path.clone(),
					fingerprint: record.fingerprint,
					prefab,
				},
				[1.0, 0.0, 0.0],
				Default::default(),
			)
			.unwrap();
		fs::write(
			source.join("scenes/main.json"),
			session.authored_document().to_json().unwrap(),
		)
		.unwrap();
		fs::write(
			source.join("prefabs/source.json"),
			b"invalid external source edits are outside the pin",
		)
		.unwrap();
		assert!(
			prepare_project(&project, &source).is_ok(),
			"live source contents are not read for a pinned instance"
		);
		fs::remove_file(source.join("prefabs/source.json")).unwrap();
		let authored = session.authored_document();
		let warnings = crate::inspect_scene_dependencies(
			&project,
			&project.startup_scene_id,
			&authored,
			&source,
		)
		.unwrap();
		assert_eq!(warnings.len(), 1);
		let warning = &warnings[0];
		assert_eq!(warning.code, "prop_editing_source_unavailable");
		assert_eq!(warning.severity, crate::DependencySeverity::Warning);
		assert_eq!(
			warning.target.as_ref().unwrap().path.as_deref(),
			Some("prefabs/source.json")
		);
		assert!(matches!(&warning.open,
			Some(crate::DependencyOpen::Prop {key}) if key == &warning.owner.as_ref().unwrap().key));

		let prepared = prepare_project(&project, &source).unwrap();
		let staged = stage_prepared_project(&prepared, fixture.0.to_str().unwrap()).unwrap();
		validate_export_snapshot(&staged, None).unwrap();
		let exported = staged.path().join("assets");
		let scene = read_scene_file(&exported.join("scenes/main.json")).unwrap();
		assert!(
			scene
				.entities
				.iter()
				.all(|entity| entity.linked_prefab.is_none())
		);
		assert!(
			scene
				.entities
				.iter()
				.any(|entity| entity.name == "Pinned prop")
		);
		assert!(!exported.join("prefabs/source.json").exists());
		assert!(exported.join("models/cube.glb").is_file());
		let catalog = dependencies::read_catalog(&exported).unwrap();
		assert_eq!(catalog.assets.len(), 1);
		let second = fixture.0.join("second");
		fs::create_dir(&second).unwrap();
		let unchanged = stage_prepared_project(&prepared, second.to_str().unwrap()).unwrap();
		assert!(
			prepare_assets(&exported).unwrap()
				== prepare_assets(&unchanged.path().join("assets")).unwrap(),
			"identical dependencies and digests"
		);
		let third = fixture.0.join("third");
		fs::create_dir(&third).unwrap();

		let mut changed = (*session.authored_document()).clone();
		changed
			.entities
			.iter_mut()
			.find(|entity| entity.linked_prefab.is_some())
			.unwrap()
			.name = "Changed anchor".into();
		fs::write(source.join("scenes/main.json"), changed.to_json().unwrap()).unwrap();
		assert!(stage_prepared_project(&prepared, third.to_str().unwrap()).is_err());
		assert!(!third.join(&project.name).exists());
	}

	#[test]
	fn runtime_spawner_missing_sources_and_dependency_cycles_fail_before_output() {
		let fixture = AssetFixture::new();
		let source = fixture.0.join("source");
		let (project, mut scene) = authored_scene_fixture(&source);
		scene.entities.push(spawner("controller", "prefabs/a.json"));
		fs::write(source.join("scenes/main.json"), scene.to_json().unwrap()).unwrap();
		assert!(prepare_project(&project, &source).is_err());
		fs::create_dir(source.join("prefabs")).unwrap();
		for (file, next) in [("a.json", "b.json"), ("b.json", "a.json")] {
			let prefab = PrefabData {
				schema_version: 3,
				composition: Default::default(),
				entities: vec![spawner("owner", &format!("prefabs/{next}"))],
				gameplay: None,
				reference_slots: Default::default(),
			};
			fs::write(source.join("prefabs").join(file), prefab.to_json().unwrap()).unwrap();
		}
		assert!(
			prepare_project(&project, &source)
				.err()
				.unwrap()
				.contains("Cyclic runtime prefab")
		);
		assert!(!fixture.0.join(&project.name).exists());
	}

	#[test]
	fn component_demo_exports_recursive_runtime_prefabs_in_canonical_format() {
		let fixture = AssetFixture::new();
		let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../apps/game");
		let project = ProjectData::from_json(&test_project::manifest("component-lab")).unwrap();
		let prepared = prepare_project(&project, &root.join("assets")).unwrap();
		let staged = stage_prepared_project(&prepared, fixture.0.to_str().unwrap()).unwrap();
		validate_export_snapshot(&staged, None).unwrap();
		let assets = staged.path().join("assets");
		assert!(assets.join("props/robot.glb").is_file());
		let value: serde_json::Value = serde_json::from_slice(
			&fs::read(assets.join("prefabs/timed-collectible.json")).unwrap(),
		)
		.unwrap();
		assert_eq!(value["schema_version"], 3);
		assert!(value["entities"][0].get("components").is_some());
		assert!(value["entities"][0].get("component_views").is_none());
	}

	#[test]
	fn runtime_prefab_alias_export_uses_canonical_source_and_matching_catalog_fingerprint() {
		let fixture = AssetFixture::new();
		let source = fixture.0.join("source");
		let (project, mut scene) = authored_scene_fixture(&source);
		let model = model_record(&source, "models/item.glb", "cube.glb");
		fs::create_dir(source.join("prefabs")).unwrap();
		let prefab = PrefabData {
			schema_version: 1,
			composition: Default::default(),
			entities: vec![
				EntityData::new(EntityKey::new("item").unwrap(), "Item", &model.path)
					.with_asset_id(Some(model.id.clone())),
			],
			gameplay: None,
			reference_slots: Default::default(),
		};
		let bytes = serde_json::to_vec(&prefab).unwrap();
		fs::write(source.join("prefabs/moved.json"), &bytes).unwrap();
		let record = PersistentAsset {
			id: AssetId::generate(),
			path: "prefabs/moved.json".into(),
			kind: AssetKind::Prefab,
			aliases: vec!["prefabs/old.json".into()],
			revision: 1,
			fingerprint: format!("{:x}", Sha256::digest(&bytes)),
			bytes: bytes.len() as u64,
			source_path: None,
			meshes: vec![],
			dependencies: vec![model.id.clone()],
		};
		save_catalog(&source, vec![model, record.clone()]);
		scene.entities.push(spawner("factory", "prefabs/old.json"));
		fs::write(source.join("scenes/main.json"), scene.to_json().unwrap()).unwrap();
		let prepared = prepare_project(&project, &source).unwrap();
		let staged = stage_prepared_project(&prepared, fixture.0.to_str().unwrap()).unwrap();
		let assets = staged.path().join("assets");
		let output = fs::read(assets.join("prefabs/moved.json")).unwrap();
		let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
		assert_eq!(value["schema_version"], 3);
		assert!(value["entities"][0].get("components").is_some());
		let catalog = dependencies::read_catalog(&assets).unwrap();
		let exported = catalog.by_id(&record.id).unwrap();
		assert_eq!(
			exported.fingerprint,
			format!("{:x}", Sha256::digest(&output))
		);
		assert_eq!(exported.bytes, output.len() as u64);
		assert_eq!(catalog.by_path("prefabs/old.json").unwrap().id, record.id);
		validate_export_snapshot(&staged, None).unwrap();
	}
}
