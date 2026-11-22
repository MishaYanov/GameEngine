use super::*;
use runtime::{
	animation::{
		AnimationClipRef, AnimatorBinding, AnimatorCondition, AnimatorGraph, AnimatorSettings,
		AnimatorState, AnimatorTransition, AnimatorVisualTarget, AuthoredClip, AuthoredClipOwner,
		ClipKey, ScopedTrack, VisualTargetId, VisualTrack,
	},
	assets::project_assets::{prepare_animation_clip, prepare_animator},
	components::{ComponentData, ComponentId, ComponentRecord, NO_MESH_PATH},
};

#[test]
fn one_animator_asset_validates_each_actors_visual_children() {
	let fixture = Fixture::new();
	let scene_path = fixture.0.join("scenes/main.json");
	let mut scene = runtime::scene_io::load_data(&scene_path).unwrap().data;
	let key = |value: &str| EntityKey::new(value).unwrap();
	for (actor, child) in [("actor-a", "arm-a"), ("actor-b", "arm-b")] {
		scene
			.entities
			.push(EntityData::new(key(actor), actor, NO_MESH_PATH).with_renderable(false));
		scene.entities.push(
			EntityData::new(key(child), child, "models/triangle.glb").with_parent(Some(key(actor))),
		);
	}
	fs::write(&scene_path, scene.to_json().unwrap()).unwrap();
	let mut workspace = fixture.open();
	let scene_id = workspace.backend.session().active_scene_id().clone();
	let channel = VisualTargetId::Scene {
		entity_key: key("arm-a"),
	};
	let clip = AuthoredClip {
		schema_version: 1,
		name: "Lift".into(),
		owner: AuthoredClipOwner::Scene {
			scene_id: scene_id.clone(),
		},
		duration_ticks: 1000,
		tracks: vec![ScopedTrack {
			target: channel.clone(),
			track: VisualTrack::LocalPosition {
				interpolation: Default::default(),
				keys: vec![
					ClipKey {
						id: None,
						at: 0,
						value: [0.0; 3],
					},
					ClipKey {
						id: None,
						at: 1000,
						value: [0.0, 2.0, 0.0],
					},
				],
			},
		}],
		markers: Vec::new(),
	};
	let clip_asset = prepare_animation_clip(&fixture.0, "lift.clip.json", &clip, None)
		.unwrap()
		.publish()
		.unwrap();
	let state = |name: &str| AnimatorState {
		id: name.to_lowercase(),
		name: name.into(),
		looping: false,
		clip: AnimationClipRef::Authored {
			asset_id: clip_asset.id.clone(),
			expected_sha256: clip_asset.fingerprint.clone(),
		},
	};
	let graph = AnimatorGraph {
		schema_version: 1,
		entry: "idle".into(),
		states: vec![state("Idle"), state("Walk")],
		parameters: Vec::new(),
		transitions: vec![AnimatorTransition {
			id: "start".into(),
			from: "idle".into(),
			to: "walk".into(),
			condition: AnimatorCondition::Always,
			exit_after_ticks: Some(8),
			blend_ticks: 4,
		}],
		layout: Default::default(),
	};
	let asset = prepare_animator(&fixture.0, "lift.animator.json", &graph, None)
		.unwrap()
		.publish()
		.unwrap();
	for (actor, child) in [("actor-a", "arm-a"), ("actor-b", "arm-b")] {
		run(
			&mut workspace,
			Command::AddComponent {
				key: actor.to_owned(),
				component: ComponentRecord {
					id: ComponentId::generate(),
					schema_version: 1,
					enabled: true,
					data: ComponentData::Animator {
						settings: AnimatorSettings {
							asset_id: asset.id.clone(),
							binding: AnimatorBinding::Visual {
								targets: vec![AnimatorVisualTarget {
									channel: channel.clone(),
									entity_key: key(child),
								}],
							},
						},
					},
				},
			},
		);
	}
	let document = workspace.backend.session().authored_document();
	for (actor, child) in [("actor-a", "arm-a"), ("actor-b", "arm-b")] {
		let actor = document
			.entities
			.iter()
			.find(|entry| entry.key.as_str() == actor)
			.unwrap();
		let settings = &actor.animator.as_ref().unwrap().settings;
		let prepared = bridge::animator_graph::prepare_actor(
			&fixture.0,
			workspace.backend.session().project(),
			&scene_id,
			&document,
			actor,
			settings,
		)
		.unwrap();
		assert_eq!(*prepared.graph, graph);
		let bridge::animator_graph::PreparedAnimatorClips::Visual(clips) = prepared.clips else {
			panic!("expected visual clips");
		};
		let pose = clips[0].sample(500).unwrap();
		assert_eq!(pose[0].entity_key, key(child));
		assert_eq!(pose[0].position, [0.0, 1.0, 0.0]);
	}
	workspace
		.save_all(workspace.backend.session().revision())
		.unwrap();
	let error = workspace
		.prepare_build(workspace.backend.session().revision())
		.err()
		.unwrap();
	assert!(
		error.contains("actor-a") && error.contains("fixed-step Animator evaluator"),
		"{error}"
	);
}
