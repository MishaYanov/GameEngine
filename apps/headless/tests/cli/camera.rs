use super::*;

#[test]
fn version_two_orbit_replay_exposes_the_same_pose_across_frame_groupings() {
	let fixture = Fixture::new();
	fixture.configure_orbit(0.0);
	let start = json!({"type":"command", "command":{"type":"start_play"}});
	let sample = json!({"type":"camera_input","yaw_delta":0.25,"pitch_delta":-0.125,"log_distance_delta":-0.5});
	let invoke = |version, actions| {
		fixture
			.command()
			.arg("--replay")
			.arg(fixture.replay_version(version, actions))
			.output()
			.unwrap()
	};
	let baseline = success(invoke(1, vec![start.clone()]));
	let edit = success(invoke(1, vec![]));
	assert!(edit.get("camera_runtime").unwrap().is_null());
	assert_eq!(edit["camera_runtime_hash_bytes"], 4);
	assert_eq!(edit["camera_runtime_hash_scope"], "camera_runtime_v1");
	assert_eq!(edit["camera_runtime_hash_algorithm"], "fnv1a64");
	assert_eq!(edit["camera_runtime_hash"].as_str().unwrap().len(), 16);
	let queued = success(invoke(
		2,
		vec![
			start.clone(),
			sample.clone(),
			json!({"type":"tick","elapsed_seconds":0.001}),
		],
	));
	assert_eq!(queued["orbit_camera_pose"], baseline["orbit_camera_pose"]);
	assert_eq!(
		queued["camera_runtime"]["orbit"],
		baseline["camera_runtime"]["orbit"]
	);
	assert_eq!(
		queued["camera_runtime"]["pending_intent"]["yaw_delta"],
		0.25
	);
	assert_ne!(
		queued["camera_runtime_hash"],
		baseline["camera_runtime_hash"]
	);
	assert_eq!(queued["state_hash"], baseline["state_hash"]);
	assert_eq!(
		queued["native_components_hash"],
		baseline["native_components_hash"]
	);
	let group = vec![
		start.clone(),
		sample.clone(),
		json!({"type":"tick","elapsed_seconds":0.05}),
	];
	let result = success(invoke(2, group.clone()));
	let repeated = success(invoke(2, group));
	let split = success(invoke(
		2,
		vec![
			start.clone(),
			sample.clone(),
			json!({"type":"tick","count":3}),
		],
	));
	let pose = &result["orbit_camera_pose"];
	assert_eq!(result["fixed_steps"], 3);
	assert_eq!(split["fixed_steps"], 3);
	assert_eq!(*pose, split["orbit_camera_pose"]);
	assert_eq!(*pose, repeated["orbit_camera_pose"]);
	for same in [&split, &repeated] {
		assert_eq!(result["camera_runtime"], same["camera_runtime"]);
		assert_eq!(result["camera_runtime_hash"], same["camera_runtime_hash"]);
	}
	assert_eq!(pose["yaw"], 0.25);
	assert_eq!(pose["pitch"], -0.125);
	let position = pose["position"].as_array().unwrap();
	let distance = position
		.iter()
		.map(|n| n.as_f64().unwrap().powi(2))
		.sum::<f64>()
		.sqrt();
	assert!((distance - 4.0 * (-0.5_f64).exp()).abs() < 1e-5);
	let mut malformed = sample.clone();
	malformed["unexpected"] = json!(true);
	assert!(
		failure(invoke(2, vec![malformed]))["error"]["message"]
			.as_str()
			.unwrap()
			.contains("unknown field")
	);
	let mut excessive = sample.clone();
	excessive["log_distance_delta"] = json!(20.0);
	assert!(
		failure(invoke(2, vec![excessive]))["error"]["message"]
			.as_str()
			.unwrap()
			.contains("4x")
	);
	let err = failure(invoke(1, vec![start, sample.clone()]));
	assert!(
		err["error"]["message"]
			.as_str()
			.unwrap()
			.contains("version 2")
	);
	let err = failure(invoke(2, vec![sample]));
	assert!(
		err["error"]["message"]
			.as_str()
			.unwrap()
			.contains("requires an unpaused orbit")
	);
}

#[test]
fn orbit_follows_a_moving_target_and_settles_without_manual_camera_input() {
	let fixture = Fixture::new();
	fixture.configure_orbit(0.15);
	let mut project = ProjectData::load(&fixture.project).unwrap();
	project
		.behaviors
		.retain(|entry| matches!(entry.behavior, BehaviorSpec::ControlledMove { .. }));
	fs::write(&fixture.project, project.to_json().unwrap()).unwrap();
	let move_actor = |x| {
		json!({"type":"command", "command":{
			"type":"set_entity_input", "entity":"actor", "movement":[x,0,0]
		}})
	};
	let run = |output, ticks, settle| {
		let state_path = fixture.root.join(output);
		let mut actions = vec![
			json!({"type":"command","command":{"type":"start_play"}}),
			move_actor(1),
			ticks,
		];
		if settle {
			actions.push(move_actor(0));
			actions.push(json!({"type":"tick", "count":180}));
		}
		let summary = success(
			fixture
				.command()
				.arg("--replay")
				.arg(fixture.replay(actions))
				.arg("--state-output")
				.arg(&state_path)
				.output()
				.unwrap(),
		);
		let world = SceneData::from_json(&fs::read_to_string(&state_path).unwrap()).unwrap();
		(summary, world.entities[0].transform.position[0])
	};
	let grouped = json!({"type":"tick", "elapsed_seconds":0.05});
	let split = json!({"type":"tick", "count":3});
	let (moving, actor_x) = run("moving-grouped.json", grouped.clone(), false);
	let (split_moving, split_x) = run("moving-split.json", split.clone(), false);
	assert_eq!(moving["fixed_steps"], 3);
	assert_eq!(actor_x, split_x);
	assert!((actor_x - 0.1).abs() < 1e-6);
	let camera = &moving["orbit_camera_pose"];
	let eye_x = camera["position"][0].as_f64().unwrap();
	assert!(eye_x > 0.0 && eye_x < f64::from(actor_x));
	assert_eq!(camera, &split_moving["orbit_camera_pose"]);
	let (settled, final_x) = run("settled-grouped.json", grouped, true);
	let (split_settled, _) = run("settled-split.json", split, true);
	let pose = &settled["orbit_camera_pose"];
	assert_eq!(pose, &split_settled["orbit_camera_pose"]);
	assert_eq!(final_x, actor_x);
	let eye: [f32; 3] = serde_json::from_value(pose["position"].clone()).unwrap();
	assert_eq!(eye, [final_x, 0.0, 4.0]);
	let steps = settled["fixed_steps"].as_u64().unwrap();
	assert!(
		(4..183).contains(&steps),
		"settled cameras stop scheduling steps"
	);
}

#[test]
fn orbit_obstruction_replay_clips_a_grazing_wall_and_recovers_after_turning() {
	let fixture = Fixture::new();
	fixture.configure_orbit(0.0);
	let mut scene = SceneData::from_json(&fs::read_to_string(&fixture.scene).unwrap()).unwrap();
	scene.camera.orbit.as_mut().unwrap().recovery_seconds = 0.2;
	// The wall spans x = 0.10..0.20, beside the center ray but inside the 0.3 m probe.
	let wall = EntityData::new(
		EntityKey::new("wall").unwrap(),
		"Wall",
		"models/not-loaded.glb",
	)
	.with_transform(TransformData {
		position: [0.15, 0.0, 3.0],
		..Default::default()
	})
	.with_collider(Some(engine_core::scene_data::ColliderData::Box {
		half_extents: [0.05, 0.5, 0.1],
		offset: [0.0; 3],
		interaction: Default::default(),
	}));
	scene.entities.push(wall);
	let source = scene.to_json().unwrap();
	fs::write(&fixture.scene, &source).unwrap();

	let replay = |turn_steps: u32| -> [f64; 3] {
		let mut actions = vec![json!({"type":"command","command":{"type":"start_play"}})];
		if turn_steps > 0 {
			actions.push(
				json!({"type":"camera_input","yaw_delta":std::f64::consts::FRAC_PI_2,
				"pitch_delta":0.0,"log_distance_delta":0.0}),
			);
			actions.push(json!({"type":"tick","count":turn_steps}));
		}
		let summary = success(
			fixture
				.command()
				.arg("--replay")
				.arg(fixture.replay_version(2, actions))
				.output()
				.unwrap(),
		);
		serde_json::from_value(summary["orbit_camera_pose"]["position"].clone()).unwrap()
	};
	let admitted = replay(0);
	assert_eq!([admitted[0], admitted[1]], [0.0; 2]);
	// The maximum geometric distance is 3 - 0.1 - 0.3 = 2.6 m, before clearance.
	assert!((2.598..2.6).contains(&admitted[2]), "{admitted:?}");
	let first = replay(1);
	let alpha = 1.0 - (-1.0_f64 / (60.0 * 0.2)).exp();
	let expected_x = -(admitted[2] + (4.0 - admitted[2]) * alpha);
	assert!(
		(first[0] - expected_x).abs() < 1e-5 && first[2].abs() < 1e-5,
		"{first:?}"
	);
	let settled = replay(180);
	assert!(
		(settled[0] + 4.0).abs() < 1e-5 && settled[2].abs() < 1e-5,
		"{settled:?}"
	);
	assert_eq!(fs::read_to_string(&fixture.scene).unwrap(), source);
}
