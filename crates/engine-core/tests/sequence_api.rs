mod support;

use engine_core::{
	SceneData,
	asset_catalog::AssetId,
	audio_clip::AudioClipRef,
	components::{
		AudioSourceComponent, AudioSourcePlayback, AudioSourceSettings, ComponentId, NO_MESH_PATH,
	},
	game_camera::{CameraComponent, CameraSettings},
	project::{BehaviorSpace, BehaviorSpec, ProjectData},
	scene_audio::AudioOnsetCause,
	scene_data::{ColliderData, ColliderInteraction},
	sequence::{
		SequenceAction, SequenceAsset, SequenceCue, SequenceEvent, SequenceInstance,
		SequenceTrigger,
	},
	simulation_session::SimulationSession,
};
use std::{collections::BTreeMap, sync::Arc};
use support::{entity, key, scene};

fn fixture(fixed_hz: u32, trigger: bool) -> (ProjectData, Arc<SceneData>) {
	let base = scene(vec![]);
	let make_camera = |name: &str, x| {
		let mut camera = entity(name, [x, 1.5, 0.]).with_renderable(false);
		camera.model_path = NO_MESH_PATH.into();
		camera.camera = Some(Box::new(CameraComponent {
			id: ComponentId::generate(),
			enabled: true,
			settings: CameraSettings::from_legacy(&base.camera),
		}));
		camera
	};
	let mut actor = entity("actor", [0.0; 3]);
	actor.collider = Some(ColliderData::Sphere {
		radius: 0.25,
		offset: [0.0; 3],
		interaction: ColliderInteraction::default(),
	});
	let mut sensor = entity("sensor", [0.0; 3]);
	sensor.collider = Some(ColliderData::Box {
		half_extents: [0.5; 3],
		offset: [0.0; 3],
		interaction: ColliderInteraction {
			sensor: true,
			..Default::default()
		},
	});
	let mut source = entity("sound-source", [0.; 3]).with_renderable(false);
	source.audio_source = Some(Box::new(AudioSourceComponent {
		id: ComponentId::generate(),
		enabled: true,
		settings: AudioSourceSettings {
			clip: Some(AudioClipRef {
				asset: AssetId::generate(),
			}),
			playback: AudioSourcePlayback::OnEvent,
			..Default::default()
		},
	}));

	let mut document = scene(vec![
		make_camera("default", 0.),
		make_camera("previous", 3.),
		make_camera("shot", 6.),
		actor,
		sensor,
		source,
	]);
	document.default_game_camera = Some(key("default"));
	let document = Arc::new(document);
	let mut project = ProjectData::for_scene("Sequence", "scenes/main.json", &document).unwrap();
	project.fixed_hz = fixed_hz;
	project.add_behavior(BehaviorSpec::ControlledMove {
		entity: key("actor"),
		speed: 5.,
		space: BehaviorSpace::World,
	});
	project.sequences.assets.push(SequenceAsset {
		id: key("camera-reveal"),
		schema_version: 1,
		duration_ms: 200,
		cues: vec![
			SequenceCue {
				at_ms: 0,
				action: SequenceAction::CameraCut { role: key("shot") },
			},
			SequenceCue {
				at_ms: 30,
				action: SequenceAction::Subtitle {
					text: "The gate is open.".into(),
					duration_ms: 130,
				},
			},
			SequenceCue {
				at_ms: 190,
				action: SequenceAction::Audio { role: key("voice") },
			},
		],
	});
	project.sequences.instances.push(SequenceInstance {
		id: key("reveal"),
		scene_id: project.startup_scene_id.clone(),
		sequence_id: key("camera-reveal"),
		bindings: BTreeMap::from([
			(key("shot"), key("shot")),
			(key("voice"), key("sound-source")),
		]),
		trigger: trigger.then(|| SequenceTrigger {
			sensor: key("sensor"),
			actor: key("actor"),
		}),
		once_per_entry: true,
	});
	(project, document)
}

#[test]
fn sensor_starts_and_finishes_a_timeline_in_place() {
	let (project, document) = fixture(20, true);
	project.validate().unwrap();
	let mut simulation = SimulationSession::new(project, document).unwrap();
	simulation
		.set_runtime_camera(simulation.runtime_handle(&key("previous")))
		.unwrap();
	simulation.set_controlled_input([1., 0., 0.]).unwrap();

	let entered = simulation.tick(0.05).unwrap();
	assert_eq!(
		entered.sequence_events,
		[SequenceEvent::Started {
			instance: key("reveal"),
			tick: 1
		}]
	);
	assert_eq!(simulation.active_camera_object().unwrap().key, key("shot"));
	assert_eq!(simulation.active_sequence(), Some(&key("reveal")));
	assert_eq!(
		simulation.world().data().entities[3].transform.position,
		[0.; 3]
	);

	simulation.set_paused(true);
	assert!(simulation.tick(0.5).unwrap().sequence_events.is_empty());
	assert_eq!(simulation.tick_index(), 1);
	simulation.set_paused(false);
	let speaking = simulation.tick(0.05).unwrap();
	assert!(
		matches!(speaking.sequence_events.as_slice(), [SequenceEvent::Subtitle {tick: 2, text, ..}] if text == "The gate is open.")
	);

	let end = simulation.tick(0.15).unwrap();
	assert!(matches!(
		end.sequence_events.as_slice(),
		[SequenceEvent::Finished { tick: 5, .. }]
	));
	assert!(matches!(
		&end.audio_onsets[0].cause,
		AudioOnsetCause::Sequence { cue: 2, .. }
	));
	assert_eq!(
		simulation.active_camera_object().unwrap().key,
		key("previous")
	);
	assert!(simulation.active_sequence().is_none());
	assert!(simulation.queue_sequence(&key("reveal")).is_err());
	assert!(simulation.tick(0.05).unwrap().sequence_events.is_empty());
	assert_eq!(
		simulation.world().data().entities[3].transform.position,
		[0.; 3]
	);
}

#[test]
fn skip_and_cancel_have_distinct_completion_and_cue_behavior() {
	let (project, document) = fixture(60, false);
	let mut simulation = SimulationSession::new(project, document).unwrap();
	let step = 1. / 60.;
	simulation.queue_sequence(&key("reveal")).unwrap();
	assert!(simulation.cancel_sequence());
	let aborted = simulation.tick(step).unwrap();
	assert!(matches!(
		aborted.sequence_events.as_slice(),
		[
			SequenceEvent::Started { .. },
			SequenceEvent::Cancelled { .. }
		]
	));
	simulation.queue_sequence(&key("reveal")).unwrap();
	assert!(simulation.skip_sequence());
	let skipped = simulation.tick(step).unwrap();
	assert!(matches!(
		skipped.sequence_events.as_slice(),
		[SequenceEvent::Started { .. }, SequenceEvent::Skipped { .. }]
	));
	assert!(skipped.audio_onsets.is_empty());
	assert_eq!(
		simulation.active_camera_object().unwrap().key,
		key("default")
	);
	assert!(simulation.queue_sequence(&key("reveal")).is_err());
}

#[test]
fn bindings_are_checked_and_sequence_time_uses_milliseconds() {
	let (mut project, document) = fixture(100, false);
	let mut invalid = project.clone();
	invalid.sequences.instances[0]
		.bindings
		.insert(key("shot"), key("actor"));
	assert!(
		invalid
			.sequences
			.validate_scene(&invalid.startup_scene_id, &document)
			.is_err()
	);
	project.sequences.assets[0].cues.swap(0, 1);
	assert!(project.validate().is_err());
	project.sequences.assets[0].cues.swap(0, 1);
	let mut simulation = SimulationSession::new(project, document).unwrap();
	simulation.queue_sequence(&key("reveal")).unwrap();
	for i in 1..=21 {
		let outcome = simulation.tick(0.01).unwrap();
		let terminal = outcome
			.sequence_events
			.iter()
			.any(|event| matches!(event, SequenceEvent::Finished { .. }));
		assert_eq!(terminal, i == 21);
	}
}
