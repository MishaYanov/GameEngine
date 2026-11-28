mod support;

use engine_core::{
	Bounds3, Key, SceneData,
	animation::{
		AnimationAsset, AnimationClipRef, AnimationSourceRef, AnimatorBinding, AnimatorComponent,
		AnimatorCondition, AnimatorGraph, AnimatorResolver, AnimatorSettings, AnimatorState,
		AnimatorTransition, Clip, Interpolation, LocalTrs, PreparedAnimator,
		PreparedAnimatorBindings, PreparedAnimatorClips, RigNode, Skin, Track, TrackValues,
	},
	asset_catalog::AssetId,
	character_controller::{CharacterControllerComponent, CharacterControllerSettings},
	command::{EditorCommandDto, EditorCommandRequest},
	components::ComponentId,
	editor::EditorSession,
	engine::EngineBuilder,
	input::{GameInputBinding, InputProfile},
	project::ProjectData,
	scene_catalog::SceneId,
	scene_data::ColliderData,
	simulation_session::{
		ActionWriter, GameModule, GameModuleFactory, InputFrame, SimulationAction,
	},
	world::RuntimeWorld,
};
use glam::Mat4;
use std::sync::{
	Arc,
	atomic::{AtomicBool, Ordering},
};
use support::{entity, key, scene};

const STEP: f64 = 1.0 / 60.0;

fn collider(half_extents: [f32; 3]) -> ColliderData {
	ColliderData::Box {
		half_extents,
		offset: [0.; 3],
		interaction: Default::default(),
	}
}

fn fixture(
	root_motion_source_node: Option<u32>,
	looping: bool,
	blend_ticks: Option<u16>,
	blocked: bool,
) -> (SceneData, ProjectData, Arc<PreparedAnimator>) {
	let source = AnimationSourceRef {
		asset_id: AssetId::generate(),
		expected_sha256: "a".repeat(64),
		scene_index: 0,
		mesh_node: 1,
		skin_index: 0,
		character: None,
	};
	let mut actor = entity("actor", [0., 0.901, 0.]).with_collider(Some(collider([0.3, 0.9, 0.3])));
	actor.asset_id = Some(source.asset_id.clone());
	actor.character_controller = Some(CharacterControllerComponent {
		id: ComponentId::new("motor").unwrap(),
		enabled: true,
		settings: CharacterControllerSettings::default(),
	});
	actor.animator = Some(Box::new(AnimatorComponent {
		id: ComponentId::new("animator").unwrap(),
		enabled: true,
		settings: AnimatorSettings {
			asset_id: AssetId::generate(),
			binding: AnimatorBinding::Imported {
				source: source.clone(),
			},
			root_motion_source_node,
			foot_ik: None,
		},
	}));

	let mut entities = vec![
		actor,
		entity("floor", [0., -0.5, 0.]).with_collider(Some(collider([20., 0.5, 20.]))),
	];
	if blocked {
		entities.push(entity("wall", [0.8, 2., 0.]).with_collider(Some(collider([0.15, 2., 5.]))));
	}
	let data = scene(entities);
	let mut project = ProjectData::for_scene("Root Motion", "scenes/main.json", &data).unwrap();
	project.input_profile = Some(InputProfile {
		actions: vec![GameInputBinding {
			id: "jump".into(),
			key: Key::Space,
			animation_parameter: None,
		}],
		..Default::default()
	});
	let state = |id: &str, clip_index| AnimatorState {
		id: id.into(),
		name: id.into(),
		clip: AnimationClipRef::Imported {
			source: source.clone(),
			clip_index,
		},
		looping,
	};
	let mut graph = AnimatorGraph {
		schema_version: 1,
		entry: "walk".into(),
		states: vec![state("walk", 1)],
		parameters: vec![],
		transitions: vec![],
		layout: Default::default(),
	};
	if let Some(blend) = blend_ticks {
		graph.entry = "idle".into();
		graph.states.push(state("idle", 0));
		graph.transitions.push(AnimatorTransition {
			id: "start".into(),
			from: "idle".into(),
			to: "walk".into(),
			condition: AnimatorCondition::Always,
			exit_after_ticks: None,
			blend_ticks: blend,
		});
	}
	let rest = LocalTrs {
		translation: [0.; 3],
		rotation_xyzw: [0., 0., 0., 1.],
		scale: [1.; 3],
	};
	let asset = AnimationAsset {
		source_sha256: source.expected_sha256.clone(),
		source_scene: 0,
		character: None,
		nodes: vec![
			RigNode {
				source_node: 0,
				name: None,
				parent: None,
				rest,
			},
			RigNode {
				source_node: 1,
				name: None,
				parent: Some(0),
				rest,
			},
		],
		skin: Skin {
			mesh_node: 1,
			source_mesh_node: 1,
			source_skin: 0,
			mesh_index: 0,
			joints: vec![1],
			inverse_bind: vec![Mat4::IDENTITY],
		},
		clips: vec![
			Clip {
				source_index: 0,
				name: "Idle".into(),
				duration_seconds: 0.1,
				tracks: vec![],
			},
			Clip {
				source_index: 1,
				name: "Walk".into(),
				duration_seconds: 0.1,
				tracks: vec![Track {
					node: 0,
					interpolation: Interpolation::Linear,
					times: vec![0., 0.1],
					values: TrackValues::Translation(vec![[2., 0., 0.], [2.2, 0.05, 0.]]),
				}],
			},
		],
		primitives: vec![],
		envelopes: vec![Some(
			Bounds3::new([-0.15, -0.8, -0.15], [0.15, 0.8, 0.15]).unwrap(),
		)],
		images: vec![],
	};
	let prepared = PreparedAnimator {
		graph: Arc::new(graph),
		clips: PreparedAnimatorClips::Imported(Arc::new(asset)),
	};
	(data, project, Arc::new(prepared))
}

fn resolver(animator: Arc<PreparedAnimator>) -> AnimatorResolver {
	Arc::new(move |_, _, _| {
		Ok(PreparedAnimatorBindings::from([(
			key("actor"),
			Arc::clone(&animator),
		)]))
	})
}

fn prepare_editor(
	scene: SceneData,
	project: ProjectData,
	animator: Arc<PreparedAnimator>,
) -> EditorSession {
	let mut editor = EditorSession::for_project(scene, project).unwrap();
	editor.set_animator_resolver(resolver(animator)).unwrap();
	editor
}

fn start(editor: &mut EditorSession) -> Result<(), String> {
	editor
		.execute_command(EditorCommandRequest {
			expected_revision: editor.revision(),
			command: EditorCommandDto::StartPlay,
		})
		.map(|_| ())
}

fn x(editor: &EditorSession) -> f32 {
	editor.render_world().unwrap().data().entities[0]
		.transform
		.position[0]
}

#[test]
fn imported_planar_motion_has_one_displacement_per_tick_and_wraps_without_a_jump() {
	let (scene, project, animator) = fixture(Some(0), true, None, false);
	let mut editor = prepare_editor(scene.clone(), project, animator);
	start(&mut editor).unwrap();
	editor
		.execute_command(EditorCommandRequest {
			expected_revision: editor.revision(),
			command: EditorCommandDto::SetEntityInput {
				entity: "actor".into(),
				movement: [1., 0., 0.],
			},
		})
		.unwrap();
	for tick in 1..=18 {
		editor.tick(STEP).unwrap();
		assert!(
			(x(&editor) - (tick as f32 / 30.)).abs() < 0.0001,
			"tick {tick}: {x}",
			x = x(&editor)
		);
		let pose = editor.animations().unwrap().pose(&key("actor")).unwrap();
		assert_eq!(pose.elapsed_ticks, tick);
		assert_eq!(pose.local[0].translation[0], 0.0);
		assert!(pose.bounds.min()[0] > x(&editor) - 0.151);
	}
	assert!(
		editor
			.animations()
			.unwrap()
			.pose(&key("actor"))
			.unwrap()
			.local[0]
			.translation[1]
			> 0.0
	);
	assert_eq!(*editor.authored_document(), scene);
}

#[test]
fn wall_blocks_root_motion_and_the_published_pose_stays_with_the_resolved_body() {
	let (scene, project, animator) = fixture(Some(0), true, None, true);
	let mut editor = prepare_editor(scene, project, animator);
	start(&mut editor).unwrap();
	for _ in 0..36 {
		editor.tick(STEP).unwrap();
	}
	let stopped = x(&editor);
	assert!((0.34..=0.351).contains(&stopped), "{stopped}");
	let pose = editor.animations().unwrap().pose(&key("actor")).unwrap();
	assert!(pose.bounds.max()[0] <= 0.502);
	assert_eq!(pose.elapsed_ticks, 36);
	assert!(editor.runtime_inspection(0, 128).unwrap().character_motion[0].planar_speed < 0.0001);
}

#[test]
fn blend_and_non_looping_root_motion_match_across_frame_partitions() {
	let (scene, project, animator) = fixture(Some(0), false, Some(4), false);
	let mut a = prepare_editor(scene.clone(), project.clone(), Arc::clone(&animator));
	let mut player_path = EngineBuilder::new(scene, project)
		.animator_resolver(resolver(animator))
		.build()
		.unwrap();
	start(&mut a).unwrap();
	player_path
		.execute(EditorCommandRequest {
			expected_revision: player_path.session().revision(),
			command: EditorCommandDto::StartPlay,
		})
		.unwrap();
	for _ in 0..12 {
		a.tick(STEP).unwrap();
	}
	for _ in 0..4 {
		player_path.tick(STEP * 3.0).unwrap();
	}
	let b = player_path.session();
	assert_eq!(x(&a), x(b));
	assert!((x(&a) - 0.15).abs() < 0.0001, "{}", x(&a));
	let pose_a = a.animations().unwrap().pose(&key("actor")).unwrap();
	let pose_b = b.animations().unwrap().pose(&key("actor")).unwrap();
	assert_eq!(pose_a.local, pose_b.local);
	assert_eq!(pose_a.elapsed_ticks, 11);
	assert_eq!(pose_a.elapsed_ticks, pose_b.elapsed_ticks);
}

#[test]
fn legacy_pose_only_behavior_is_unchanged_and_wrong_rig_root_fails_before_play() {
	let (scene, project, animator) = fixture(None, true, None, false);
	let mut editor = prepare_editor(scene, project, animator);
	start(&mut editor).unwrap();
	editor.tick(STEP).unwrap();
	assert_eq!(x(&editor), 0.0);
	assert!(
		editor
			.animations()
			.unwrap()
			.pose(&key("actor"))
			.unwrap()
			.local[0]
			.translation[0]
			> 0.03
	);

	let (scene, project, animator) = fixture(Some(1), true, None, false);
	let mut rejected = prepare_editor(scene.clone(), project, animator);
	let error = start(&mut rejected).unwrap_err();
	assert!(error.contains("rig root"), "{error}");
	assert!(!rejected.is_playing());
	assert_eq!(*rejected.authored_document(), scene);
}

#[test]
fn in_place_or_excessively_fast_clips_cannot_be_used_as_root_motion() {
	for fast in [false, true] {
		let (scene, project, mut animator) = fixture(Some(0), true, None, false);
		let PreparedAnimatorClips::Imported(asset) =
			&mut Arc::get_mut(&mut animator).unwrap().clips
		else {
			panic!("fixture requires imported animation");
		};
		let clip = &mut Arc::get_mut(asset).unwrap().clips[1];
		let expected = if fast {
			clip.tracks[0].values = TrackValues::Translation(vec![[0., 0., 0.], [1000., 0., 0.]]);
			"100 units per second"
		} else {
			clip.tracks.clear();
			"no planar translation"
		};
		let mut editor = prepare_editor(scene, project, animator);
		let error = start(&mut editor).unwrap_err();
		assert!(error.contains(expected), "{error}");
		assert!(!editor.is_playing());
	}
}

#[derive(Clone, Default)]
struct RejectStep(Arc<AtomicBool>);

impl GameModuleFactory for RejectStep {
	fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
		Ok(Box::new(self.clone()))
	}
}

impl GameModule for RejectStep {
	fn needs_tick(&self, _: &InputFrame) -> bool {
		self.0.load(Ordering::Relaxed)
	}

	fn plan(
		&self,
		_: &RuntimeWorld,
		_: &InputFrame,
		_: f32,
		output: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		if self.0.load(Ordering::Relaxed) {
			output.push(SimulationAction::SetVisible {
				entity: key("missing"),
				visible: false,
			})?;
		}
		Ok(())
	}
}

#[test]
fn rejected_tick_rolls_back_both_root_displacement_and_playback_clock() {
	let (scene, project, animator) = fixture(Some(0), true, None, false);
	let mut editor = prepare_editor(scene, project, animator);
	let reject = RejectStep::default();
	editor.set_module_factory(Arc::new(reject.clone())).unwrap();
	start(&mut editor).unwrap();
	editor.tick(STEP).unwrap();
	let before = x(&editor);
	reject.0.store(true, Ordering::Relaxed);
	assert!(editor.tick(STEP).is_err());
	assert_eq!(x(&editor), before);
	assert_eq!(
		editor
			.animations()
			.unwrap()
			.pose(&key("actor"))
			.unwrap()
			.elapsed_ticks,
		1
	);
}

#[test]
fn timeline_actor_pose_temporarily_overrides_animator_root_motion() {
	use engine_core::{
		game_session::GameSession,
		sequence::{SequenceAction, SequenceAsset, SequenceCue, SequenceEvent, SequenceInstance},
		simulation_session::ConfiguredBehaviorFactory,
	};
	use std::collections::BTreeMap;

	let (scene, mut project, animator) = fixture(Some(0), true, None, false);
	let scene_id = project.startup_scene_id.clone();
	project.sequences.assets.push(SequenceAsset {
		id: key("cinematic"),
		schema_version: 1,
		duration_ms: 500,
		cues: vec![SequenceCue {
			at_ms: 0,
			action: SequenceAction::PlayClip {
				role: key("performer"),
				clip_index: 0,
			},
		}],
	});
	project.sequences.instances.push(SequenceInstance {
		id: key("arrival"),
		scene_id: scene_id.clone(),
		sequence_id: key("cinematic"),
		bindings: BTreeMap::from([(key("performer"), key("actor"))]),
		trigger: None,
		once_per_entry: true,
	});
	let mut game = GameSession::with_factory_for_scene(
		Arc::new(project),
		scene_id,
		Arc::new(scene),
		Arc::new(ConfiguredBehaviorFactory),
	)
		.unwrap();
	game.set_animator_resolver(resolver(animator)).unwrap();
	game.active_mut().queue_sequence(&key("arrival")).unwrap();
	let started = game.tick(STEP).unwrap();
	assert!(matches!(
		started.sequence_events.as_slice(),
		[SequenceEvent::Started { .. }]
	));
	for _ in 0..5 {
		game.tick(STEP).unwrap();
		let simulation = game.active();
		assert_eq!(
			simulation.world().data().entities[0].transform.position[0],
			0.
		);
		assert_eq!(
			simulation
				.animations()
				.pose(&key("actor"))
				.unwrap()
				.clip_index,
			0
		);
	}
	assert!(game.active_mut().skip_sequence());
	assert!(matches!(
		game.tick(STEP).unwrap().sequence_events.as_slice(),
		[SequenceEvent::Skipped { .. }]
	));
	game.tick(STEP).unwrap();
	let simulation = game.active();
	assert!(simulation.world().data().entities[0].transform.position[0] > 0.);
	assert_eq!(
		simulation
			.animations()
			.pose(&key("actor"))
			.unwrap()
			.clip_index,
		1
	);
}
