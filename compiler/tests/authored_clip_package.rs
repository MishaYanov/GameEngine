use engine_core::{
	EntityKey,
	animation::{
		AuthoredClip, AuthoredClipOwner, ClipKey, ScopedTrack, VisualInterpolation, VisualTargetId,
		VisualTrack,
	},
	asset_catalog::{AssetCatalog, AssetId, AssetKind, PersistentAsset},
	components::NO_MESH_PATH,
	editor::PrefabData,
	linked_prefab::PropAddress,
	project::ProjectData,
	scene_data::{EntityData, SceneData},
};
use sha2::{Digest, Sha256};
use std::{
	fs,
	path::{Path, PathBuf},
};

struct Fixture(PathBuf);

impl Fixture {
	fn new() -> Self {
		let root = Path::new(env!("CARGO_MANIFEST_DIR"))
			.join("../target")
			.join(format!("qa-authored-clip-{}", AssetId::generate().as_str()));
		fs::create_dir_all(&root).unwrap();
		Self(root)
	}
}

impl Drop for Fixture {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

fn record(
	id: AssetId,
	path: &str,
	kind: AssetKind,
	bytes: &[u8],
	dependencies: Vec<AssetId>,
) -> PersistentAsset {
	PersistentAsset {
		id,
		path: path.into(),
		kind,
		aliases: vec![],
		revision: 1,
		fingerprint: format!("{:x}", Sha256::digest(bytes)),
		bytes: bytes.len() as u64,
		source_path: None,
		meshes: vec![],
		dependencies,
	}
}

#[test]
fn packages_registered_clip_with_its_prop_and_rejects_missing_file_or_object() {
	let fixture = Fixture::new();
	let original = Path::new(env!("CARGO_MANIFEST_DIR")).join("../apps/game/assets");
	let assets = fixture.0.join("assets");
	fs::create_dir_all(assets.join("props")).unwrap();
	fs::create_dir_all(assets.join("prefabs")).unwrap();
	fs::copy(
		original.join("props/cube.glb"),
		assets.join("props/cube.glb"),
	)
	.unwrap();
	let mut scene = SceneData::from_json(
		&fs::read_to_string(original.join("scenes/animation-room.json")).unwrap(),
	)
	.unwrap();
	scene.entities.clear();
	scene.default_game_camera = None;
	scene.bindings.clear();
	scene.camera.follow = None;
	scene.camera.orbit = None;
	fs::write(assets.join("room.json"), scene.to_json().unwrap()).unwrap();
	let project = ProjectData::for_scene("Visual Clip", "room.json", &scene).unwrap();

	let frame = EntityKey::new("frame").unwrap();
	let lever = EntityKey::new("lever").unwrap();
	let prefab = PrefabData {
		schema_version: 3,
		entities: vec![
			EntityData::new(frame.clone(), "Frame", NO_MESH_PATH).with_renderable(false),
			EntityData::new(lever.clone(), "Lever", "props/cube.glb").with_parent(Some(frame)),
		],
		gameplay: None,
		reference_slots: Default::default(),
		composition: Default::default(),
	};
	let prefab_bytes = prefab.to_json().unwrap().into_bytes();
	fs::write(assets.join("lever.json"), &prefab_bytes).unwrap();
	let prop_id = AssetId::generate();
	let clip_id = AssetId::generate();
	let mut clip = AuthoredClip {
		schema_version: 1,
		name: "Lever Turn".into(),
		owner: AuthoredClipOwner::Prop {
			asset_id: prop_id.clone(),
		},
		duration_ticks: 1_000,
		tracks: vec![ScopedTrack {
			target: VisualTargetId::Prop {
				address: PropAddress::Object {
					placement_path: vec![],
					source_entity: lever,
				},
			},
			track: VisualTrack::LocalPosition {
				interpolation: VisualInterpolation::Linear,
				keys: vec![
					ClipKey {
						at: 0,
						value: [0.0; 3],
					},
					ClipKey {
						at: 1000,
						value: [2.0, 0.0, 0.0],
					},
				],
			},
		}],
	};
	let clip_path = assets.join("prefabs/lever-turn.clip.json");
	let clip_bytes = clip.to_json().unwrap();
	fs::write(&clip_path, &clip_bytes).unwrap();
	let mut catalog = AssetCatalog::default();
	catalog
		.replace(record(
			prop_id.clone(),
			"lever.json",
			AssetKind::Prefab,
			&prefab_bytes,
			vec![],
		))
		.unwrap();
	catalog
		.replace(record(
			clip_id.clone(),
			"prefabs/lever-turn.clip.json",
			AssetKind::AnimationClip,
			&clip_bytes,
			vec![prop_id.clone()],
		))
		.unwrap();
	fs::write(
		assets.join("asset-catalog.json"),
		catalog.to_json().unwrap(),
	)
	.unwrap();
	let prepared = compiler::prepare_project(&project, &assets).unwrap();
	let output = fixture.0.join("output");
	fs::create_dir(&output).unwrap();
	let staged = compiler::stage_prepared_project(&prepared, output.to_str().unwrap()).unwrap();
	let exported = staged.path().join("assets");
	assert_eq!(
		fs::read(exported.join("prefabs/lever-turn.clip.json")).unwrap(),
		clip_bytes
	);
	assert!(exported.join("lever.json").is_file());
	assert!(exported.join("props/cube.glb").is_file());

	clip.tracks[0].target = VisualTargetId::Prop {
		address: PropAddress::Object {
			placement_path: vec![],
			source_entity: EntityKey::new("missing-lever").unwrap(),
		},
	};
	let invalid = clip.to_json().unwrap();
	fs::write(&clip_path, &invalid).unwrap();
	catalog
		.replace(record(
			clip_id,
			"prefabs/lever-turn.clip.json",
			AssetKind::AnimationClip,
			&invalid,
			vec![prop_id],
		))
		.unwrap();
	fs::write(
		assets.join("asset-catalog.json"),
		catalog.to_json().unwrap(),
	)
	.unwrap();
	let error = compiler::prepare_project(&project, &assets).err().unwrap();
	assert!(error.contains("missing-lever"), "{error}");
	fs::remove_file(&clip_path).unwrap();
	let error = compiler::prepare_project(&project, &assets).err().unwrap();
	assert!(error.contains("prefabs/lever-turn.clip.json"), "{error}");
}
