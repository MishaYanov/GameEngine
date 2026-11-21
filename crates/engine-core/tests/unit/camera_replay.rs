use super::*;

#[test]
fn orbit_projection_exposes_settling_retained_pose_and_instance_identity() {
	let camera = Camera::default();
	let mut orbit = OrbitRuntimeState {
		target: RuntimeEntityHandle {
			key: EntityKey::new("actor").unwrap(),
			generation: 5,
			world_id: "session-a".into(),
		},
		target_lost: true,
		yaw: 0.0,
		pitch: -0.0,
		user_distance: 5.0,
		effective_distance: 2.5,
		filtered_pivot: [0.0; 3],
		previous_target_pivot: [0.0; 3],
		last_allowed_distance: 3.0,
		obstruction: None,
		pivot_settled: true,
		arm_settled: false,
		tick: 19,
	};
	let project = |state: &OrbitRuntimeState| {
		let p = CameraReplayProjection::from_committed(
			&camera,
			Some(state),
			false,
			CameraIntent::default(),
			"session-a",
		)
		.unwrap();
		serde_json::to_value(p).unwrap()
	};
	let initial = project(&orbit);
	assert_eq!(initial["mode"], "orbit");
	assert_eq!(
		initial["orbit"]["target"],
		serde_json::json!({"key":"actor","generation":5,"same_world":true})
	);
	assert_eq!(
		initial["pose"]["position"],
		serde_json::json!(camera.position)
	);
	assert_eq!(initial["orbit"]["target_lost"], true);
	orbit.arm_settled = true;
	let settled = project(&orbit);
	assert_ne!(initial, settled);
	assert_eq!(settled["orbit"]["arm_settled"], true);
	orbit.effective_distance = 3.0;
	assert_ne!(settled, project(&orbit));
	orbit.target.world_id = "different-world".into();
	assert_eq!(project(&orbit)["orbit"]["target"]["same_world"], false);
}

#[test]
fn fixed_and_follow_publish_finite_poses_and_normalize_negative_zero() {
	let mut camera = Camera {
		pitch: -0.0,
		..Default::default()
	};
	for (follow, name) in [(false, "fixed"), (true, "follow")] {
		let replay = CameraReplayProjection::from_committed(
			&camera,
			None,
			follow,
			CameraIntent::default(),
			"world",
		)
		.unwrap();
		assert_eq!(serde_json::to_value(&replay.mode).unwrap(), name);
		assert_eq!(replay.pose.pitch.to_bits(), 0);
		assert!(replay.orbit.is_none());
	}
	camera.position[0] = f32::INFINITY;
	assert!(
		CameraReplayProjection::from_committed(
			&camera,
			None,
			false,
			CameraIntent::default(),
			"world"
		)
		.is_err()
	);
}
