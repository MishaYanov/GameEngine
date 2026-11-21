use super::*;

#[test]
fn f5_saves_before_motion_resets_clock_and_requires_a_fresh_press_to_save_again() {
	let directory = TestDirectory::new();
	let path = directory.0.join("robots.json");
	let (mut app, [left, _]) = fixture();
	app.scene_path = Some(path.clone());
	app.project_path = Some(directory.0.join("project.json"));
	let before = snapshot(&app);
	let start = prime_clock(&mut app);
	app.input.press(Key::F5);
	app.update(0.5);
	assert_eq!(snapshot(&app), before);
	let saved_bytes = fs::read(&path).unwrap();
	let saved = SceneData::from_json(&String::from_utf8(saved_bytes.clone()).unwrap()).unwrap();
	assert_eq!(saved.to_json().unwrap(), before);
	assert_eq!(
		app.clock.tick(start + Duration::from_secs(60)),
		Duration::ZERO
	);
	for key in [Key::F5, Key::W, Key::ArrowRight] {
		assert!(app.input.is_held(key));
	}

	app.input.end_frame();
	app.input.press(Key::F5); // Repeated key-down while held is not another save.
	app.update(0.1);
	assert!((app.scene.get_mut(left).unwrap().transform().position[1] - 0.1).abs() < 1e-5);
	assert!((app.camera.position[0] - 0.2).abs() < 1e-5);
	assert_eq!(fs::read(&path).unwrap(), saved_bytes);

	app.input.release(Key::F5);
	app.input.end_frame();
	app.input.press(Key::F5);
	let moved = snapshot(&app);
	app.update(0.1);
	assert_eq!(snapshot(&app), moved);
	let saved = SceneData::from_json(&fs::read_to_string(&path).unwrap()).unwrap();
	assert_eq!(saved.to_json().unwrap(), moved);
}

#[test]
fn failed_f5_attempt_still_suppresses_frame_motion_and_resets_clock() {
	let (mut app, _) = fixture();
	let before = snapshot(&app);
	let start = prime_clock(&mut app);
	app.input.press(Key::F5);
	app.update(0.5); // No scene path has been configured.
	assert_eq!(snapshot(&app), before);
	assert_eq!(
		app.clock.tick(start + Duration::from_secs(60)),
		Duration::ZERO
	);
	assert!(app.input.is_held(Key::W));
	assert!(app.input.is_held(Key::ArrowRight));
}

#[test]
fn f9_takes_priority_over_f5_and_failed_reload_preserves_disk_and_live_state() {
	let directory = TestDirectory::new();
	let path = directory.0.join("robots.json");
	fs::write(&path, "external edits must survive").unwrap();
	let (mut app, [left, right]) = fixture();
	app.scene_path = Some(path.clone());
	app.project_path = Some(directory.0.join("project.json"));
	app.asset_root = Some(directory.0.clone());
	let before = snapshot(&app);
	let start = prime_clock(&mut app);
	for key in [Key::F9, Key::F5, Key::Tab] {
		app.input.press(key);
	}
	app.update(0.5); // Missing renderer makes reload fail before installation.
	assert_eq!(snapshot(&app), before);
	assert_eq!(
		fs::read_to_string(&path).unwrap(),
		"external edits must survive"
	);
	assert_eq!(
		app.clock.tick(start + Duration::from_secs(60)),
		Duration::ZERO
	);
	for key in [Key::F9, Key::F5, Key::W, Key::ArrowRight] {
		assert!(app.input.is_held(key));
	}

	app.input.end_frame();
	app.input.press(Key::F9);
	app.input.press(Key::F5);
	app.update(0.1);
	assert!((app.scene.get_mut(left).unwrap().transform().position[1] - 0.1).abs() < 1e-5);
	assert_eq!(
		app.scene.get_mut(right).unwrap().transform().position,
		[0.8, 0.0, 0.0]
	);
	assert!((app.camera.position[0] - 0.2).abs() < 1e-5);
	assert_eq!(
		fs::read_to_string(&path).unwrap(),
		"external edits must survive"
	);
}

#[test]
fn saving_during_play_updates_authored_baseline_only_after_successful_disk_write() {
	let directory = TestDirectory::new();
	let (mut app, _) = fixture();
	app.input.reset();
	app.tools.saved = Some(app.scene.to_data(&app.camera).unwrap());
	let old_baseline = app.tools.saved.as_ref().unwrap().to_json().unwrap();
	app.editor
		.rename_selected(&mut app.scene, "Authored edit")
		.unwrap();
	app.camera.yaw = 0.4;
	let authored = snapshot(&app);
	app.start_play().unwrap();
	press(&mut app, &[Key::W, Key::ArrowRight], 0.5);
	let simulated = snapshot(&app);
	assert_ne!(simulated, authored);
	assert!(app.is_dirty());
	app.scene_path = Some(directory.0.join("missing/robots.json"));
	app.project_path = Some(directory.0.join("project.json"));
	assert!(app.save_scene().is_err());
	assert_eq!(
		app.tools.saved.as_ref().unwrap().to_json().unwrap(),
		old_baseline
	);
	assert!(app.is_dirty());
	let path = directory.0.join("robots.json");
	app.scene_path = Some(path.clone());
	app.project_path = Some(directory.0.join("project.json"));
	app.save_scene().unwrap();
	assert_eq!(
		app.tools.saved.as_ref().unwrap().to_json().unwrap(),
		authored
	);
	assert!(!app.is_dirty());
	assert_eq!(snapshot(&app), simulated);
	assert_eq!(
		SceneData::from_json(&fs::read_to_string(path).unwrap())
			.unwrap()
			.to_json()
			.unwrap(),
		authored
	);
	app.stop_play().unwrap();
	assert_eq!(snapshot(&app), authored);
	assert!(!app.is_dirty());
	press(&mut app, &[Key::ControlLeft, Key::Z], 0.0);
	assert!(app.is_dirty());
}

#[test]
fn project_save_failure_after_scene_write_keeps_authored_dirty_baselines_for_retry() {
	let directory = TestDirectory::new();
	let (mut app, [left, _]) = fixture();
	app.input.reset();
	app.tools.saved = Some(app.scene.to_data(&app.camera).unwrap());
	let baseline = app.tools.saved.as_ref().unwrap().to_json().unwrap();
	app.scene_path = Some(directory.0.join("robots.json"));
	let blocked = directory.0.join("blocked-project.json");
	fs::create_dir(&blocked).unwrap();
	app.project_path = Some(blocked);
	app.editor
		.rename_selected(&mut app.scene, "Authored retry")
		.unwrap();
	let edited = snapshot(&app);
	assert!(app.is_dirty());
	let error = app.save_scene().unwrap_err();
	assert!(error.contains("Scene was saved"), "{error}");
	assert!(error.contains("remains dirty"));
	assert_eq!(
		SceneData::from_json(&fs::read_to_string(app.scene_path.as_ref().unwrap()).unwrap())
			.unwrap()
			.to_json()
			.unwrap(),
		edited
	);
	assert_eq!(
		app.tools.saved.as_ref().unwrap().to_json().unwrap(),
		baseline
	);
	assert!(app.is_dirty());
	assert_eq!(app.scene.name(left), Some("Authored retry"));
	app.project_path = Some(directory.0.join("project.json"));
	app.save_scene().unwrap();
	assert!(!app.is_dirty());
	let project = runtime::project::ProjectData::load(app.project_path.as_ref().unwrap()).unwrap();
	project
		.validate_scene(&app.scene.to_data(&app.camera).unwrap())
		.unwrap();
}
