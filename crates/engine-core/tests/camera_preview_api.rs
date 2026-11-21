mod support;

use std::sync::Arc;

use engine_core::{
	Camera, SceneData,
	camera_input::CameraIntent,
	camera_preview::FrozenOrbitPreview,
	game_camera::OrbitCameraSettings,
	game_session::GameSession,
	project::ProjectData,
	scene_data::{ColliderData, EntityData},
	simulation_session::ConfiguredBehaviorFactory,
};
use support::{entity, key, scene};

const STEP: f32 = 1.0 / 60.0;

fn solid(name: &str, position: [f32; 3]) -> EntityData {
	entity(name, position).with_collider(Some(ColliderData::Box {
		half_extents: [0.25; 3],
		offset: [0.0; 3],
		interaction: Default::default(),
	}))
}

fn fixture(wall_z: f32) -> (Arc<SceneData>, OrbitCameraSettings) {
	let actor = solid("actor", [0.0; 3]);
	let mut child = solid("child", [0.0; 3]);
	child.parent = Some(actor.key.clone());
	let settings = OrbitCameraSettings {
		target: actor.key.clone(),
		pivot_offset: [0.0; 3],
		initial_yaw: 0.0,
		initial_pitch: 0.0,
		initial_distance: 5.0,
		min_distance: 1.0,
		max_distance: 10.0,
		min_pitch: -1.0,
		max_pitch: 1.0,
		follow_seconds: 0.0,
		recovery_seconds: 0.1,
		teleport_distance: 10.0,
		probe_radius: 0.3,
		obstruction_mask: 1,
	};
	let data = scene(vec![actor, child, solid("wall", [0.0, 0.0, wall_z])]);
	(Arc::new(data), settings)
}

#[test]
fn frozen_draft_obstruction_and_recovery_match_play_without_editing_the_document() {
	let (document, settings) = fixture(2.0);
	let original = document.to_json().unwrap();
	let preview = FrozenOrbitPreview::from_document(
		Arc::clone(&document),
		settings.clone(),
		Camera::default(),
		STEP,
	)
	.unwrap();
	let initial = preview.orbit_state();
	assert_eq!(initial.obstruction.as_ref().unwrap().blocker, key("wall"));
	assert!(initial.effective_distance > 1.4 && initial.effective_distance < 1.45);

	let mut applied = (*document).clone();
	applied.camera.orbit = Some(settings);
	let project = ProjectData::for_scene("Camera parity", "scene.json", &applied).unwrap();
	let startup = project.startup_scene_id.clone();
	let mut play = GameSession::with_factory_for_scene(
		Arc::new(project),
		startup,
		Arc::new(applied),
		Arc::new(ConfiguredBehaviorFactory),
	)
	.unwrap();
	assert_eq!(preview.pose().position, play.active().camera().position);
	let turn = CameraIntent {
		yaw_delta: std::f64::consts::FRAC_PI_2,
		..Default::default()
	};
	let next = preview.advance(turn).unwrap();
	let owner = play.camera_input_owner().unwrap();
	play.submit_camera_input(&owner.run_id, owner.scene_generation, turn)
		.unwrap();
	assert_eq!(play.tick(1.0 / 60.0).unwrap().steps, 1);
	assert!(next.orbit_state().obstruction.is_none());
	assert!(next.orbit_state().effective_distance > initial.effective_distance);
	assert!(next.orbit_state().effective_distance < initial.user_distance);
	assert_eq!(next.pose().position, play.active().camera().position);
	assert_eq!(next.pose().yaw, play.active().camera().yaw);
	assert_eq!(preview.orbit_state().tick, 0);
	assert_eq!(document.to_json().unwrap(), original);
}

#[test]
fn invalid_origin_and_input_do_not_replace_an_accepted_preview() {
	let (document, settings) = fixture(2.0);
	let open = |doc, interval| {
		FrozenOrbitPreview::from_document(doc, settings.clone(), Camera::default(), interval)
	};
	let accepted = open(document.clone(), STEP).unwrap();
	assert!(
		open(fixture(0.0).0, STEP)
			.err()
			.unwrap()
			.contains("penetrating")
	);
	assert!(open(document, 0.0).is_err());
	let initial = accepted.orbit_state().clone();
	let invalid = CameraIntent {
		yaw_delta: f64::NAN,
		..Default::default()
	};
	assert!(accepted.advance(invalid).is_err());
	assert_eq!(accepted.orbit_state(), &initial);
	assert_eq!(
		accepted
			.advance(CameraIntent::default())
			.unwrap()
			.orbit_state()
			.tick,
		1
	);
}

#[test]
fn preview_query_budget_rejection_preserves_the_last_pose_and_tick() {
	let (mut document, settings) = fixture(2.0);
	// The initial +Z arm skips these boxes in the broad phase. Turning toward
	// +X visits all of them twice, including the rounded-endpoint verification.
	Arc::make_mut(&mut document)
		.entities
		.extend((0..4100).map(|i| solid(&format!("obstacle-{i}"), [2.0, 0.0, 0.0])));
	let preview =
		FrozenOrbitPreview::from_document(document, settings, Camera::default(), STEP).unwrap();
	let original_eye = preview.pose().position;
	let turn = CameraIntent {
		yaw_delta: -std::f64::consts::FRAC_PI_2,
		..Default::default()
	};
	assert!(
		preview
			.advance(turn)
			.err()
			.unwrap()
			.contains("8192 work visits")
	);
	assert_eq!(preview.pose().position, original_eye);
	assert_eq!(preview.orbit_state().tick, 0);
	assert_eq!(
		preview
			.advance(CameraIntent::default())
			.unwrap()
			.orbit_state()
			.tick,
		1
	);
}
