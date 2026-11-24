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
							root_motion_source_node: None,
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
	let revision = workspace.backend.session().revision();
	let preview = workspace
		.open_authored_clip_preview(
			&clip_asset.id,
			&clip_asset.fingerprint,
			None,
			revision,
			workspace.generation,
			1,
		)
		.unwrap();
	let sample = workspace
		.sample_authored_clip_preview(&preview.token, 500)
		.unwrap();
	let frame = workspace.active_authored_clip_aperture().unwrap();
	assert_eq!(
		frame.visual_pose.unwrap().poses.as_ref(),
		sample.poses.as_slice()
	);
	let (saved, preview_closed) =
		workspace.with_viewport_update(|workspace| workspace.save_all(revision));
	saved.unwrap();
	assert!(preview_closed);
	assert!(workspace.active_authored_clip_aperture().is_none());
	let authored = fs::read(&scene_path).unwrap();
	let prepared = workspace
		.prepare_build(workspace.backend.session().revision())
		.unwrap();
	let output = fixture.0.parent().unwrap().join("animator-package");
	fs::create_dir(&output).unwrap();
	let staged = compiler::stage_prepared_project(&prepared, output.to_str().unwrap()).unwrap();
	let project = runtime::project::ProjectData::load(staged.path().join("project.json")).unwrap();
	let root = staged.path().join("assets");
	let packaged_scene = runtime::scene_io::load_data(&root.join("scenes/main.json"))
		.unwrap()
		.data;
	let mut headless = runtime::simulation_session::SimulationSession::new(
		project,
		std::sync::Arc::new(packaged_scene),
	)
	.unwrap();
	headless
		.set_animator_resolver(std::sync::Arc::new(move |project, id, scene| {
			bridge::animator_graph::prepare_scene(&root, project, id, scene)
		}))
		.unwrap();
	run(&mut workspace, Command::StartPlay);
	let step = 1.0 / 60.0;
	for _ in 0..4 {
		workspace.backend.tick(step * 4.0).unwrap();
	}
	for _ in 0..16 {
		headless.tick(step).unwrap();
	}
	let editor_bank = workspace.backend.session().animations().unwrap();
	let native_bank = headless.animations();
	assert_eq!(editor_bank.visual_matrices(), native_bank.visual_matrices());
	let actual = editor_bank.animator_inspection();
	assert_eq!(actual.len(), 2);
	assert!(actual
		.iter()
		.all(|entry| entry.state_id == "walk" && entry.state_ticks == 7));
	assert!(editor_bank.visual_matrices()[&key("arm-a")].w_axis.y > 0.1);
	assert_eq!(headless.document().entities, document.entities);
	run(&mut workspace, Command::StopPlay);
	assert_eq!(fs::read(&scene_path).unwrap(), authored);
}
