use super::*;

#[test]
fn authored_chapters_advance_independently_and_restart_from_the_first_course() {
	let fixture = Fixture::new();
	let game = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../game");
	let summary = success(
		Command::new(env!("CARGO_BIN_EXE_engine-headless"))
			.current_dir(&fixture.root)
			.arg("--project")
			.arg(game.join("project.courtyard-chapters.json"))
			.arg("--replay")
			.arg(game.join("fixtures/courtyard-chapters/route.replay.json"))
			.output()
			.unwrap(),
	);
	let markers = summary["markers"].as_array().unwrap();
	for (label, scene, chapter, score, key, gate, complete) in [
		(
			"courtyard_entry",
			"courtyard",
			Some("courtyard"),
			0,
			false,
			false,
			false,
		),
		(
			"courtyard_gate",
			"courtyard",
			Some("courtyard"),
			1,
			true,
			true,
			false,
		),
		(
			"garden_entry",
			"garden",
			Some("garden"),
			1,
			false,
			false,
			false,
		),
		(
			"garden_key",
			"garden",
			Some("garden"),
			2,
			true,
			false,
			false,
		),
		("finish_entry", "finish", None, 2, false, false, false),
		("finished", "finish", None, 2, false, false, true),
		(
			"restarted",
			"courtyard",
			Some("courtyard"),
			0,
			false,
			false,
			false,
		),
	] {
		let row = markers
			.iter()
			.find(|marker| marker["label"] == label)
			.unwrap();
		assert_eq!(row["scene_id"], scene, "{row}");
		assert_eq!(
			row.get("chapter_id").and_then(Value::as_str),
			chapter,
			"{row}"
		);
		assert_eq!(row["score"], score, "{row}");
		assert_eq!(row["has_courtyard_key"], key, "{row}");
		assert_eq!(row["route_gate_open"], gate, "{row}");
		assert_eq!(row["completed"], complete, "{row}");
	}
	assert_eq!(markers.len(), 7);
}

#[test]
fn renamed_and_repaired_campaign_batch_replays_identically_after_compiler_staging() {
	use courtyard_gameplay::authoring_repairs::distinct_sensor_repairs;
	use engine_core::{scene_catalog::SceneId, scene_data::ColliderData};

	let fixture = Fixture::new();
	let game = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../game");
	let mut authored = ProjectData::load(&game.join("project.courtyard-chapters.json")).unwrap();
	authored.gameplay_settings.as_mut().unwrap()["campaign"]["chapters"][1]["label"] =
		"Garden Walk".into();
	fs::write(&fixture.project, authored.to_json().unwrap()).unwrap();
	let assets = fixture.root.join("assets");
	for scene in authored.scenes.iter() {
		fs::copy(
			game.join("assets").join(scene.asset_path()),
			assets.join(scene.asset_path()),
		)
		.unwrap();
	}
	fs::create_dir(assets.join("props")).unwrap();
	for file in ["capsule.glb", "cube.glb", "sphere.glb"] {
		fs::copy(
			game.join("assets/props").join(file),
			assets.join("props").join(file),
		)
		.unwrap();
	}
	let garden_path = assets.join("scenes/chapter-garden.json");
	let mut garden = SceneData::from_json(&fs::read_to_string(&garden_path).unwrap()).unwrap();
	for key in ["garden-key-trigger", "garden-exit-trigger"] {
		let entity = garden
			.entities
			.iter_mut()
			.find(|entity| entity.key.as_str() == key)
			.unwrap();
		match entity.collider.as_mut().unwrap() {
			ColliderData::Box { interaction, .. } | ColliderData::Sphere { interaction, .. } => {
				interaction.sensor = false;
			}
		}
	}
	fs::write(&garden_path, garden.to_json().unwrap()).unwrap();
	assert!(compiler::prepare_project(&authored, &assets).is_err());
	let garden_id = SceneId::new("garden").unwrap();
	let repairs = distinct_sensor_repairs(&authored, &garden_id, &garden);
	assert_eq!(repairs.len(), 2);
	for repair in repairs {
		let entity = garden
			.entities
			.iter_mut()
			.find(|entity| entity.key == repair.target.entity_key)
			.unwrap();
		entity.collider = Some(repair.after);
	}
	fs::write(&garden_path, garden.to_json().unwrap()).unwrap();
	let prepared = compiler::prepare_project(&authored, &assets).unwrap();
	let staged =
		compiler::stage_prepared_project(&prepared, fixture.root.to_str().unwrap()).unwrap();
	let packaged_manifest = staged.path().join("project.json");
	assert_eq!(ProjectData::load(&packaged_manifest).unwrap(), authored);
	let replay = game.join("fixtures/courtyard-chapters/route.replay.json");
	let run = |project: &std::path::Path| {
		success(
			Command::new(env!("CARGO_BIN_EXE_engine-headless"))
				.current_dir(&fixture.root)
				.arg("--project")
				.arg(project)
				.arg("--replay")
				.arg(&replay)
				.output()
				.unwrap(),
		)
	};
	let source = run(&fixture.project);
	let packaged = run(&packaged_manifest);
	assert_eq!(source["markers"], packaged["markers"]);
	assert_eq!(packaged["markers"][5]["label"], "finished");
	assert_eq!(packaged["markers"][5]["score"], 2);
	assert_eq!(packaged["markers"][5]["completed"], true);
}

#[test]
fn release_learning_route_reaches_finish_after_one_deliberate_press() {
	let fixture = Fixture::new();
	let game = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../game");
	let project = game.join("project.release-learning.json");
	let script: Value = serde_json::from_str(
		&fs::read_to_string(game.join("fixtures/courtyard-release-learning/route.replay.json"))
			.unwrap(),
	)
	.unwrap();
	let actions = script["actions"].as_array().unwrap();
	let run = |label: &str, actions: Vec<Value>| {
		let replay = fixture.replay_version(3, actions);
		let state = fixture.root.join(format!("{label}.json"));
		let summary = success(
			Command::new(env!("CARGO_BIN_EXE_engine-headless"))
				.current_dir(&fixture.root)
				.arg("--project")
				.arg(&project)
				.arg("--replay")
				.arg(replay)
				.arg("--state-output")
				.arg(&state)
				.output()
				.unwrap(),
		);
		let scene = SceneData::from_json(&fs::read_to_string(state).unwrap()).unwrap();
		(summary, scene)
	};
	let gate = |scene: &SceneData| {
		let gate = scene
			.entities
			.iter()
			.find(|entity| entity.key.as_str() == "courtyard-route-gate")
			.unwrap();
		(gate.renderable, gate.collision_enabled)
	};

	let (summary, approach) = run("approach", actions[..5].to_vec());
	assert_eq!(summary["play_status"]["gameplay"]["score"], 1);
	assert_eq!(gate(&approach), (true, true));

	let (_, opened) = run("opened", actions[..8].to_vec());
	assert_eq!(gate(&opened), (false, false));

	let (summary, checkpoint) = run("checkpoint", actions[..11].to_vec());
	assert_eq!(summary["play_scene_id"], "courtyard");
	let player = checkpoint
		.entities
		.iter()
		.find(|entity| entity.key.as_str() == "courtyard-player")
		.unwrap();
	assert!((player.transform.position[0] + 0.97).abs() < 0.12);

	let (summary, _) = run("finished", actions.clone());
	assert_eq!(summary["active_scene_id"], "courtyard");
	assert_eq!(summary["play_scene_id"], "finish");
	assert_eq!(summary["play_status"]["gameplay"]["score"], 1);
	assert_eq!(summary["play_status"]["gameplay"]["completed"], true);

	let mut restarted = actions.clone();
	restarted.push(json!({"type":"restart_run"}));
	let (summary, restarted) = run("restarted", restarted);
	assert_eq!(summary["play_scene_id"], "courtyard");
	assert_eq!(summary["play_status"]["gameplay"]["score"], 0);
	assert_eq!(gate(&restarted), (true, true));
}

#[test]
fn release_learning_route_rejects_duplicate_over_limit_and_legacy_keyboard_records() {
	let fixture = Fixture::new();
	for (version, keys, expected) in [
		(2, json!(["KeyE"]), "requires replay schema version 3"),
		(
			3,
			json!(["KeyE", "KeyE"]),
			"requires distinct physical key codes",
		),
		(
			3,
			json!((0..17).map(|index| format!("F{index}")).collect::<Vec<_>>()),
			"accepts at most 16 keys",
		),
	] {
		let replay = fixture.replay_version(
			version,
			vec![json!({
				"type": "keyboard_input",
				"keys": keys,
			})],
		);
		let result = failure(
			fixture
				.command()
				.arg("--replay")
				.arg(replay)
				.output()
				.unwrap(),
		);
		assert!(
			result["error"]["message"]
				.as_str()
				.unwrap()
				.contains(expected),
			"{result}"
		);
	}
}

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
