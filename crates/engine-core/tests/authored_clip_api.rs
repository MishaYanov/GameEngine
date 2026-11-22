mod support;

use engine_core::{
	animation::{
		AnimationClipRef, AuthoredClip, AuthoredClipOwner, ClipKey, ScopedTrack,
		VisualInterpolation, VisualTargetId, VisualTrack,
	},
	asset_catalog::AssetId,
	editor::{EditorSession, PrefabData},
	linked_prefab::{LinkedPrefabSource, PropAddress},
	project::ProjectData,
	scene_catalog::SceneId,
	scene_data::ColliderData,
};
use support::{entity, key, scene};

fn clip(owner: AuthoredClipOwner, target: VisualTargetId) -> AuthoredClip {
	AuthoredClip {
		schema_version: 1,
		name: "Lever Turn".into(),
		owner,
		duration_ticks: 1_000,
		markers: Vec::new(),
		tracks: vec![
			ScopedTrack {
				target: target.clone(),
				track: VisualTrack::LocalPosition {
					interpolation: VisualInterpolation::Linear,
					keys: vec![
						ClipKey {
							id: None,
							at: 0,
							value: [0.0; 3],
						},
						ClipKey {
							id: None,
							at: 1000,
							value: [2.0, 4.0, 0.0],
						},
					],
				},
			},
			ScopedTrack {
				target,
				track: VisualTrack::LocalRotation {
					interpolation: VisualInterpolation::Linear,
					keys: vec![
						ClipKey {
							id: None,
							at: 0,
							value: [0.0, 0.0, 0.0, 1.0],
						},
						ClipKey {
							id: None,
							at: 1000,
							value: [0.0, 1.0, 0.0, 0.0],
						},
					],
				},
			},
		],
	}
}

#[test]
fn authored_clip_roundtrips_and_resolves_to_distinct_placed_instances() {
	let prop = AssetId::generate();
	let lever = entity("lever", [0.0; 3]).with_parent(Some(key("frame")));
	let data = PrefabData {
		schema_version: 3,
		entities: vec![entity("frame", [0.0; 3]), lever],
		gameplay: None,
		reference_slots: Default::default(),
		composition: Default::default(),
	};
	let authored = clip(
		AuthoredClipOwner::Prop {
			asset_id: prop.clone(),
		},
		VisualTargetId::Prop {
			address: PropAddress::Object {
				placement_path: vec![],
				source_entity: key("lever"),
			},
		},
	);
	let authored = AuthoredClip::from_json(&authored.to_json().unwrap()).unwrap();
	let local = authored
		.resolve_prop(&prop, &data)
		.unwrap()
		.sample(500)
		.unwrap()
		.remove(0);
	assert_eq!(local.position, [1.0, 2.0, 0.0]);
	let half_turn = std::f32::consts::FRAC_1_SQRT_2;
	assert!((local.rotation_xyzw[1] - half_turn).abs() < 1.0e-6);
	assert!((local.rotation_xyzw[3] - half_turn).abs() < 1.0e-6);

	let source = LinkedPrefabSource {
		source_id: prop,
		path: "lever.json".into(),
		fingerprint: "a".repeat(64),
		prefab: data,
	};
	let mut session = EditorSession::new(scene(vec![])).unwrap();
	for offset in [0.0, 10.0] {
		session
			.instantiate_linked_prefab(source.clone(), [offset, 0.0, 0.0], Default::default())
			.unwrap();
	}
	let document = session.authored_document();
	let scene_id = session.active_scene_id().clone();
	let project = session.project();
	let roots: Vec<_> = document
		.entities
		.iter()
		.filter(|entity| entity.linked_prefab.is_some())
		.collect();
	let poses: Vec<_> = roots
		.iter()
		.map(|root| {
			authored
				.resolve_prop_instance(&project, &scene_id, &document, &root.key)
				.unwrap()
				.sample(500)
				.unwrap()
				.remove(0)
		})
		.collect();
	assert_ne!(poses[0].entity_key, poses[1].entity_key);
	assert_eq!(poses[0].position, poses[1].position);
	assert_eq!(session.authored_document(), document);
}

#[test]
fn rejects_unsorted_keys_incompatible_scopes_and_gameplay_dependent_descendants() {
	let scene_id = SceneId::new("room").unwrap();
	let owner = AuthoredClipOwner::Scene {
		scene_id: scene_id.clone(),
	};
	let target = VisualTargetId::Scene {
		entity_key: key("lever"),
	};
	let mut authored = clip(owner, target);
	if let VisualTrack::LocalPosition { keys, .. } = &mut authored.tracks[0].track {
		keys[1].at = keys[0].at;
	}
	assert!(authored.to_json().unwrap_err().contains("strictly ordered"));
	if let VisualTrack::LocalPosition {
		keys,
		interpolation,
	} = &mut authored.tracks[0].track
	{
		keys[1].at = 1000;
		*interpolation = VisualInterpolation::Hold;
	}
	let mut data = scene(vec![
		entity("frame", [0.0; 3]),
		entity("lever", [0.0; 3]).with_parent(Some(key("frame"))),
	]);
	let project =
		ProjectData::for_scene_with_id("Room", scene_id.clone(), "room.json", &data).unwrap();
	assert!(
		authored
			.resolve_scene(&project, &SceneId::new("other").unwrap(), &data)
			.is_err()
	);
	assert_eq!(
		authored
			.resolve_scene(&project, &scene_id, &data)
			.unwrap()
			.sample(999)
			.unwrap()[0]
			.position,
		[0.0; 3]
	);

	data.entities.push(
		entity("blocker", [0.0; 3])
			.with_parent(Some(key("lever")))
			.with_collider(Some(ColliderData::Sphere {
				radius: 1.0,
				interaction: Default::default(),
				offset: [0.0; 3],
			})),
	);
	assert!(
		authored
			.resolve_scene(&project, &scene_id, &data)
			.err()
			.unwrap()
			.contains("visual-only subtree")
	);
	let bad_ref = AnimationClipRef::Authored {
		asset_id: AssetId::generate(),
		expected_sha256: "changed".into(),
	};
	assert!(bad_ref.validate().is_err());
}
