use super::*;

#[test]
fn bundled_route_reports_locked_exit_key_completion_restart_and_restored_authored_state() {
	let fixture = Fixture::new();
	let sample = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../game");
	let manifest = sample.join("project.json");
	let project = ProjectData::load(&manifest).unwrap();
	let source = sample
		.join("assets")
		.join(project.startup_scene_path().unwrap());
	let source_bytes = fs::read(&source).unwrap();
	let authored = SceneData::from_json(std::str::from_utf8(&source_bytes).unwrap()).unwrap();
	let mut actions = vec![json!({"type":"command","command":{"type":"start_play"}})];
	for (index, movement, count, scene_id, score, objective, completed) in [
		(0, -1.0, 90, "courtyard", 0, "Find the courtyard key", false),
		(
			1,
			1.0,
			190,
			"courtyard",
			1,
			"Reach the courtyard exit",
			false,
		),
		// The locked door now stops the actor at contact; leave enough travel
		// to return from the pickup and sample the authored exit sensor.
		(2, -1.0, 240, "finish", 1, "Reach the finish marker", false),
		(3, 1.0, 90, "finish", 1, "Courtyard Run complete", true),
	] {
		actions.extend([
			json!({"type":"command","command":{"type":"set_play_input","movement":[movement,0,0]}}),
			json!({"type":"tick","count":count}),
		]);
		let replay = fixture.replay(actions.clone());
		let summary = success(
			Command::new(env!("CARGO_BIN_EXE_engine-headless"))
				.current_dir(&fixture.root)
				.arg("--project")
				.arg(&manifest)
				.arg("--replay")
				.arg(replay)
				.output()
				.unwrap(),
		);
		assert_eq!(summary["active_scene_id"], "courtyard", "phase {index}");
		assert_eq!(summary["play_scene_id"], scene_id);
		assert_eq!(summary["play_status"]["scene_id"], scene_id);
		assert_eq!(summary["play_status"]["gameplay"]["score"], score);
		assert_eq!(summary["play_status"]["gameplay"]["objective"], objective);
		assert_eq!(summary["play_status"]["gameplay"]["completed"], completed);
	}
	actions.extend([
		json!({"type":"restart_run"}),
		json!({"type":"tick","count":5}),
	]);
	let replay = fixture.replay(actions.clone());
	let state = fixture.root.join("restarted.json");
	let run = |replay: &PathBuf, state: &PathBuf| {
		success(
			Command::new(env!("CARGO_BIN_EXE_engine-headless"))
				.current_dir(&fixture.root)
				.arg("--project")
				.arg(&manifest)
				.arg("--replay")
				.arg(replay)
				.arg("--state-output")
				.arg(state)
				.output()
				.unwrap(),
		)
	};
	let restarted = run(&replay, &state);
	assert_eq!(restarted["play_scene_id"], "courtyard");
	assert_eq!(restarted["play_status"]["gameplay"]["score"], 0);
	assert_eq!(restarted["play_status"]["gameplay"]["completed"], false);
	assert_eq!(
		restarted["play_status"]["gameplay"]["objective"],
		"Find the courtyard key"
	);
	assert_eq!(
		restarted["simulation_tick"], 1,
		"Restart clears held input and samples fresh sensors once"
	);
	assert_eq!(
		SceneData::from_json(&fs::read_to_string(state).unwrap()).unwrap(),
		authored
	);
	actions.push(json!({"type":"command","command":{"type":"stop_play"}}));
	let replay = fixture.replay(actions);
	let state = fixture.root.join("restored.json");
	let stopped = run(&replay, &state);
	assert_eq!(stopped["mode"], "edit");
	assert!(stopped.get("play_status").is_none());
	assert_eq!(
		SceneData::from_json(&fs::read_to_string(state).unwrap()).unwrap(),
		authored
	);
	assert_eq!(fs::read(&source).unwrap(), source_bytes);
}

#[test]
fn headless_collects_the_bound_key_without_input_only_when_the_campaign_is_selected() {
	let fixture = Fixture::new();
	let mut scene = SceneData::from_json(&fs::read_to_string(&fixture.scene).unwrap()).unwrap();
	let key = EntityKey::new("key-marker").unwrap();
	scene
		.entities
		.push(EntityData::new(key.clone(), "Key", "models/not-loaded.glb"));
	scene.bindings.extend([
		("player".into(), EntityKey::new("actor").unwrap()),
		("key".into(), key),
		("exit".into(), EntityKey::new("other").unwrap()),
	]);
	let document = scene.to_json().unwrap();
	fs::write(&fixture.scene, &document).unwrap();

	let mut project = ProjectData::load(&fixture.project).unwrap();
	project.behaviors.truncate(1);
	fs::write(&fixture.project, project.to_json().unwrap()).unwrap();
	let run = || success(fixture.command().arg("--steps").arg("2").output().unwrap());
	assert_eq!(run()["fixed_steps"], 0);

	// The player spawns next to the key. With movement idle and the exit far
	// away, the pickup is the only reason to step.
	project.gameplay_module = Some(courtyard_gameplay::COURTYARD_RUN_ID.into());
	fixture.add_finish(&mut project, &scene);
	fs::write(&fixture.project, project.to_json().unwrap()).unwrap();
	let selected = run();
	let repeated = run();
	assert_eq!(selected["fixed_steps"], 1);
	assert_eq!(selected["simulation_tick"], 1);
	assert_eq!(selected["state_hash"], repeated["state_hash"]);
	assert_eq!(selected["change_cursor"], repeated["change_cursor"]);
	assert_eq!(fs::read_to_string(&fixture.scene).unwrap(), document);
}

#[test]
fn headless_run_reaches_finish_and_stop_restores_authored_scene() {
	let fixture = Fixture::new();
	let mut scene = SceneData::from_json(&fs::read_to_string(&fixture.scene).unwrap()).unwrap();
	scene.entities[1].transform.position = [0.0; 3];
	let key = EntityKey::new("key-marker").unwrap();
	scene
		.entities
		.push(EntityData::new(key.clone(), "Key", "models/not-loaded.glb"));
	scene.bindings.extend([
		("player".into(), EntityKey::new("actor").unwrap()),
		("key".into(), key),
		("exit".into(), EntityKey::new("other").unwrap()),
	]);
	let mut project = ProjectData::load(&fixture.project).unwrap();
	project.behaviors.truncate(1);
	project.gameplay_module = Some(courtyard_gameplay::COURTYARD_RUN_ID.into());
	let finish = fixture.add_finish(&mut project, &scene);
	let authored = scene.to_json().unwrap();
	fs::write(&fixture.scene, &authored).unwrap();
	fs::write(&fixture.project, project.to_json().unwrap()).unwrap();
	let mut actions = vec![
		json!({"type":"command","command":{"type":"start_play"}}),
		json!({"type":"command","command":{"type":"set_play_input","movement":[1,0,0]}}),
		json!({"type":"tick","count":10}),
	];
	let replay = fixture.replay(actions.clone());
	let state = fixture.root.join("destination.json");
	let result = success(
		fixture
			.command()
			.arg("--replay")
			.arg(&replay)
			.arg("--state-output")
			.arg(&state)
			.output()
			.unwrap(),
	);
	assert_eq!(result["active_scene_id"], project.startup_scene_id.as_str());
	assert_eq!(result["play_scene_id"], "finish");
	assert_eq!(result["fixed_steps"], 2);
	assert_eq!(result["simulation_tick"], 0);
	let rendered = SceneData::from_json(&fs::read_to_string(&state).unwrap()).unwrap();
	assert_eq!(
		rendered, finish,
		"old held input must not move the destination player"
	);
	assert_eq!(fs::read_to_string(&fixture.scene).unwrap(), authored);

	actions.extend([
		json!({"type":"command","command":{"type":"set_play_input","movement":[1,0,0]}}),
		json!({"type":"tick","count":90}),
	]);
	let replay = fixture.replay(actions.clone());
	let completed_state = fixture.root.join("completed.json");
	let completed = success(
		fixture
			.command()
			.arg("--replay")
			.arg(&replay)
			.arg("--state-output")
			.arg(&completed_state)
			.output()
			.unwrap(),
	);
	assert_eq!(
		completed["active_scene_id"],
		project.startup_scene_id.as_str()
	);
	assert_eq!(completed["play_scene_id"], "finish");
	// Twenty movement steps enter marker range, then one completion step
	// commits without moving. The remaining held-input ticks all stay idle.
	assert_eq!(completed["simulation_tick"], 21);
	assert_eq!(completed["fixed_steps"], 23);
	let rendered = SceneData::from_json(&fs::read_to_string(completed_state).unwrap()).unwrap();
	let player_x = rendered.entities[0].transform.position[0];
	assert!((player_x - 2.0 / 3.0).abs() < 1e-5, "{player_x}");
	assert_eq!(rendered.bindings, finish.bindings);

	// Background gameplay must not make the editor's Stop revision stale.
	actions.push(json!({"type":"command","expected_revision":3,"command":{"type":"stop_play"}}));
	let replay = fixture.replay(actions);
	let stopped_state = fixture.root.join("stopped.json");
	let stopped = success(
		fixture
			.command()
			.arg("--replay")
			.arg(&replay)
			.arg("--state-output")
			.arg(&stopped_state)
			.output()
			.unwrap(),
	);
	assert_eq!(stopped["mode"], "edit");
	assert!(stopped.get("play_scene_id").is_none());
	assert_eq!(fs::read_to_string(&fixture.scene).unwrap(), authored);
	assert_eq!(
		SceneData::from_json(
			&fs::read_to_string(fixture.root.join("assets/scenes/finish.json")).unwrap()
		)
		.unwrap(),
		finish,
	);
	assert_eq!(
		SceneData::from_json(&fs::read_to_string(stopped_state).unwrap()).unwrap(),
		scene
	);
}

#[test]
fn headless_can_open_a_course_draft_but_rejects_play_without_its_exit_role() {
	let fixture = Fixture::new();
	let mut scene = SceneData::from_json(&fs::read_to_string(&fixture.scene).unwrap()).unwrap();
	scene.bindings.extend([
		("player".into(), EntityKey::new("actor").unwrap()),
		("key".into(), EntityKey::new("other").unwrap()),
	]);
	fs::write(&fixture.scene, scene.to_json().unwrap()).unwrap();
	let mut project = ProjectData::load(&fixture.project).unwrap();
	project.gameplay_module = Some(courtyard_gameplay::COURTYARD_RUN_ID.into());
	fs::write(&fixture.project, project.to_json().unwrap()).unwrap();

	let opened = success(fixture.command().arg("--steps").arg("0").output().unwrap());
	assert_eq!(opened["mode"], "edit");
	let error = failure(fixture.command().arg("--steps").arg("1").output().unwrap());
	assert!(
		error["error"]["message"]
			.as_str()
			.unwrap()
			.contains(r#"requires a "exit" binding"#)
	);
}
