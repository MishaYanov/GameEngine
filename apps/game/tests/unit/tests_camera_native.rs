//! Native camera acceptance through the same rendering and input paths as the player.
use super::*;
use serde_json::{Value, json};
use std::{fs, process::Command};
use winit::platform::windows::EventLoopBuilderExtWindows;

fn evidence_directory(label: &str) -> PathBuf {
	let nonce = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.unwrap()
		.as_nanos();
	let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("../../target")
		.join(format!(
			"camera-native-{label}-{}-{nonce}",
			std::process::id()
		));
	fs::create_dir_all(&dir).unwrap();
	dir
}

fn hidden_player(
	events: &ActiveEventLoop,
	project_path: &Path,
	[width, height]: [u32; 2],
) -> GameApp {
	let window = Arc::new(
		events
			.create_window(
				Window::default_attributes()
					.with_visible(false)
					.with_decorations(false)
					.with_inner_size(winit::dpi::PhysicalSize::new(width, height)),
			)
			.unwrap(),
	);
	let mut app = GameApp::default();
	app.project = Some(ProjectData::load(project_path).unwrap());
	app.project_path = Some(project_path.to_owned());
	app.standalone_runtime = true;
	app.installed_project = true;
	app.initialize_window(window, project_path.parent().unwrap().join("assets"))
		.unwrap();
	assert_eq!(
		app.renderer.as_mut().unwrap().prepare().unwrap(),
		Some([width, height])
	);
	app
}

fn route_checkpoints(project: &Path, dir: &Path) -> Vec<Value> {
	let route: Value = serde_json::from_slice(include_bytes!(
		"../../../../docs/verification/milestone-10-camera-route.json"
	))
	.unwrap();
	let actions = route["actions"].as_array().unwrap();
	let headless = std::env::current_exe()
		.unwrap()
		.parent()
		.unwrap()
		.parent()
		.unwrap()
		.join("engine-headless.exe");
	assert!(
		headless.is_file(),
		"Build engine-headless in the same Cargo profile first"
	);
	// Keep the independent headless oracle outside the measured native frames.
	[1, 5, 9, 11, 12]
		.into_iter()
		.map(|end| {
			let path = dir.join(format!("checkpoint-{end}.json"));
			fs::write(
				&path,
				serde_json::to_vec(&json!({"schema_version":2, "actions":&actions[..end]}))
					.unwrap(),
			)
			.unwrap();
			let output = Command::new(&headless)
				.arg("--project")
				.arg(project)
				.arg("--replay")
				.arg(path)
				.output()
				.unwrap();
			assert!(
				output.status.success(),
				"{}",
				String::from_utf8_lossy(&output.stderr)
			);
			let summary: Value = serde_json::from_slice(&output.stdout).unwrap();
			assert!(
				summary["camera_runtime"].is_object(),
				"Headless binary must expose the camera projection"
			);
			summary["camera_runtime"].clone()
		})
		.collect()
}

fn assert_camera_checkpoint(app: &GameApp, expected: &Value) {
	let state = app.editor.camera_replay_state().unwrap().unwrap();
	let actual: Value = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
	assert_eq!(&actual, expected);
	let pose = app.editor.play_rig_camera().unwrap();
	assert_eq!(state.pose.position, pose.position);
	assert_eq!((state.pose.yaw, state.pose.pitch), (pose.yaw, pose.pitch));
}

#[derive(Default)]
struct RouteTimes {
	update: Vec<f64>,
	frame: Vec<f64>,
	gpu: Vec<f64>,
}

impl RouteTimes {
	fn steps(&mut self, app: &mut GameApp, count: usize) {
		for _ in 0..count {
			let start = Instant::now();
			app.tick_play(1.0 / 60.0).unwrap();
			app.input.end_frame();
			self.update.push(start.elapsed().as_secs_f64() * 1000.0);
			assert_eq!(app.render().unwrap(), renderer::DrawStatus::Rendered);
			self.frame.push(start.elapsed().as_secs_f64() * 1000.0);
			if let Some(ms) = app.renderer.as_ref().unwrap().last_metrics().gpu_time_ms {
				self.gpu.push(ms);
			}
		}
	}
}

fn timing_summary(samples: &mut [f64]) -> Value {
	if samples.is_empty() {
		return Value::Null;
	}
	samples.sort_by(f64::total_cmp);
	let percentile = |p: f64| samples[(samples.len() as f64 * p).ceil() as usize - 1];
	json!({"samples": samples.len(), "p50_ms": percentile(0.5), "p95_ms": percentile(0.95), "max_ms": samples.last()})
}

#[test]
#[ignore = "requires native Vulkan, the camera acceptance room and same-profile engine-headless"]
fn player_camera_route_matches_headless_at_two_extents() {
	let fixture = test_project::ProjectFixture::new("camera-room");
	let project = std::env::var_os("GAME_ENGINE_QA_CAMERA_PROJECT")
		.map(PathBuf::from)
		.unwrap_or_else(|| fixture.manifest_path());
	let dir = evidence_directory("route");
	let checkpoints = route_checkpoints(&project, &dir);
	let mut reports = Vec::new();
	with_native_window(|events| {
		for extent in [[1280, 720], [1920, 1080]] {
			let mut app = hidden_player(events, &project, extent);
			let device = app.renderer.as_ref().unwrap().physical_device_info();
			for _ in 0..20 {
				assert_eq!(app.render().unwrap(), renderer::DrawStatus::Rendered);
			}
			assert_camera_checkpoint(&app, &checkpoints[0]);
			let mut times = RouteTimes::default();
			app.input.press(Key::S); // Default character input maps backward to +Z.
			times.steps(&mut app, 60);
			app.input.release(Key::S);
			times.steps(&mut app, 180);
			assert_camera_checkpoint(&app, &checkpoints[1]);
			assert_eq!(
				checkpoints[1]["orbit"]["obstruction"]["blocker"],
				"camera-wall"
			);
			app.input.press(Key::D);
			times.steps(&mut app, 60);
			app.input.release(Key::D);
			times.steps(&mut app, 240);
			assert_camera_checkpoint(&app, &checkpoints[2]);
			assert_eq!(checkpoints[2]["orbit"]["effective_distance"], 5.0);
			let owner = app.editor.camera_input_owner().unwrap();
			app.editor
				.submit_camera_input(
					&owner,
					CameraIntent {
						yaw_delta: 0.25,
						pitch_delta: -0.05,
						log_distance_delta: -0.1,
					},
				)
				.unwrap();
			times.steps(&mut app, 1);
			assert_camera_checkpoint(&app, &checkpoints[3]);
			assert_eq!(
				app.camera.position,
				app.editor.play_rig_camera().unwrap().position
			);
			app.restart_player_game().unwrap();
			assert_camera_checkpoint(&app, &checkpoints[4]);
			assert_eq!(checkpoints[0], checkpoints[4]);
			app.wait_for_gpu().unwrap();
			let (working, peak) = super::performance_tests::working_set();
			reports.push(json!({
				"extent": extent, "device": device.name, "driver_version": device.driver_version, "warmup_draws":20,
				"update": timing_summary(&mut times.update), "update_through_present":timing_summary(&mut times.frame),
				"gpu_prior_frame":timing_summary(&mut times.gpu), "working_set_bytes":working.parse::<u64>().ok(),
				"peak_process_working_set_bytes":peak.parse::<u64>().ok()
			}));
		}
	});
	let report = json!({"project":project, "debug_assertions":cfg!(debug_assertions), "reports":reports,
		"matched_camera_checkpoints":["admission","wall","recovered","turned","restart"]});
	fs::write(
		dir.join("results.json"),
		serde_json::to_vec_pretty(&report).unwrap(),
	)
	.unwrap();
	println!("Camera route passed: {}\n{report}", dir.display());
}

#[test]
#[ignore = "requires native Windows Vulkan"]
fn cinder_checkpoint_native_controls_import_review_audit_and_continue() {
	use crate::cinder_checkpoint::{CHECKPOINT_SUFFIX, PracticeStore, audit::AUDIT_SUFFIX};
	use gameplay_catalog::cinder_oath::{CinderProgress, Phase};
	use runtime::input::{FaceButton, PhysicalGamepadSample};

	const CHILD: &str = "GAME_ENGINE_QA_CINDER_CHECKPOINT_PROFILE";
	let Some(profile) = std::env::var_os(CHILD).map(PathBuf::from) else {
		let profile = evidence_directory("cinder-checkpoint");
		let output = Command::new(std::env::current_exe().unwrap())
			.args([
				"--ignored",
				"--exact",
				"camera_native_tests::cinder_checkpoint_native_controls_import_review_audit_and_continue",
				"--nocapture",
			])
			.env(CHILD, &profile)
			.env("LOCALAPPDATA", &profile)
			.output()
			.unwrap();
		assert!(
			output.status.success(),
			"{}\n{}",
			String::from_utf8_lossy(&output.stdout),
			String::from_utf8_lossy(&output.stderr),
		);
		return;
	};
	let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("../..")
		.join(gameplay_catalog::BUNDLED_PROJECT_DIRECTORY)
		.join("project.json");
	let transfer = profile.join("transfer");
	fs::create_dir(&transfer).unwrap();

	with_native_window(|events| {
		let mut app = hidden_player(events, &manifest, [960, 540]);
		app.transfer_directory = Some(transfer.clone());
		app.player_controls.configure(Some(&manifest), None, false);
		let key = |app: &mut GameApp, key| assert!(app.player_controls_key(key));
		let selected = |app: &GameApp| {
			app.player_controls_rows(3, 80)
				.unwrap()
				.into_iter()
				.find_map(|line| line.strip_prefix("> ").map(str::to_owned))
				.unwrap()
		};
		let focus = |app: &mut GameApp, label: &str| {
			for _ in 0..12 {
				if selected(app).starts_with(label) {
					return;
				}
				key(app, Key::ArrowDown);
			}
			panic!("missing Controls action {label}");
		};
		let enter = |app: &mut GameApp, label: &str| {
			focus(app, label);
			key(app, Key::Enter);
		};

		app.input.press(Key::Enter);
		app.tick_play(1.0 / 60.0).unwrap();
		app.input.release(Key::Enter);
		app.input.end_frame();
		assert_eq!(app.editor.play_scene_id().unwrap().as_str(), "arena");
		app.input.press(Key::F);
		app.tick_play(1.0 / 60.0).unwrap();
		app.input.release(Key::F);
		app.input.end_frame();
		assert_eq!(
			app.editor
				.play_progress::<CinderProgress>()
				.unwrap()
				.swings(),
			1
		);
		key(&mut app, Key::Escape);
		enter(&mut app, "Cinder Oath Checkpoints");
		enter(&mut app, "Save Arena Entry");
		enter(&mut app, "Export Checkpoint");
		assert_eq!(
			crate::local_player_files::transfer_entries(&transfer, CHECKPOINT_SUFFIX)
				.unwrap()
				.len(),
			1
		);

		enter(&mut app, "Import Checkpoint");
		enter(&mut app, "Checkpoint 01");
		assert_eq!(selected(&app), "Cancel");
		let store = PracticeStore::new(&profile, &manifest).unwrap();
		let checkpoint = store.checkpoint().unwrap().unwrap();
		store.save(checkpoint).unwrap();
		enter(&mut app, "Replace Practice Slot");
		assert!(
			app.player_controls_rows(4, 80)
				.unwrap()
				.iter()
				.any(|row| row == "Practice slot changed")
		);
		enter(&mut app, "Recheck Selected File");
		assert_eq!(selected(&app), "Cancel");
		enter(&mut app, "Replace Practice Slot");
		assert!(
			app.player_controls_rows(4, 80)
				.unwrap()
				.iter()
				.any(|row| row == "Imported into Practice slot")
		);

		focus(&mut app, "Audit filter");
		key(&mut app, Key::ArrowLeft);
		assert_eq!(selected(&app), "Audit filter: Import");
		enter(&mut app, "Export Audit");
		assert_eq!(selected(&app), "Cancel");
		enter(&mut app, "Export Reviewed Audit");
		assert_eq!(
			crate::local_player_files::transfer_entries(&transfer, AUDIT_SUFFIX)
				.unwrap()
				.len(),
			1
		);

		enter(&mut app, "Review Audit File");
		enter(&mut app, "Audit 01");
		assert_eq!(selected(&app), "Close");
		let rows = app.player_controls_rows(10, 80).unwrap();
		assert!(rows.iter().any(|row| row.contains("Direction: Import")));
		assert!(rows.iter().any(|row| row.contains("Rows: 1")));
		let mut sample = PhysicalGamepadSample::default();
		assert!(!app.player_controls_gamepad(sample));
		sample.face_buttons[FaceButton::B as usize] = true;
		assert!(app.player_controls_gamepad(sample));
		assert_eq!(selected(&app), "Review Audit File");
		assert_eq!(
			app.editor
				.play_progress::<CinderProgress>()
				.unwrap()
				.swings(),
			1
		);
		enter(&mut app, "Continue Practice Slot");
		assert!(!app.player_controls.is_open());
		let progress = app.editor.play_progress::<CinderProgress>().unwrap();
		assert_eq!(progress.phase(), Phase::Duel);
		assert_eq!(progress.swings(), 0);
		assert_eq!(app.editor.play_scene_id().unwrap().as_str(), "arena");
		assert_eq!(app.render().unwrap(), renderer::DrawStatus::Rendered);
		app.restart_player_game().unwrap();
		assert_eq!(app.editor.play_scene_id().unwrap().as_str(), "intro");
	});
}

/// Runs once inside a hidden Winit event loop on a test thread and propagates failure.
fn with_native_window(work: impl FnOnce(&ActiveEventLoop)) {
	struct Check<F>(Option<F>);
	impl<F: FnOnce(&ActiveEventLoop)> ApplicationHandler for Check<F> {
		fn resumed(&mut self, events: &ActiveEventLoop) {
			self.0.take().unwrap()(events);
			events.exit();
		}
		fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
	}
	let events = EventLoop::builder().with_any_thread(true).build().unwrap();
	let mut check = Check(Some(work));
	events.run_app(&mut check).unwrap();
	assert!(check.0.is_none());
}

#[test]
#[ignore = "requires native Windows Vulkan and the Vulkan SDK screenshot layer"]
fn cinder_oath_native_player_completes_intro_and_sword_duel() {
	use gameplay_catalog::cinder_oath::{CinderProgress, Phase};
	use winit::keyboard::KeyCode;

	const CHILD: &str = "GAME_ENGINE_QA_CINDER_OATH_CAPTURE";
	if std::env::var_os(CHILD).is_some() {
		let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
			.join("../..")
			.join(gameplay_catalog::BUNDLED_PROJECT_DIRECTORY)
			.join("project.json");
		with_native_window(|events| {
			let mut app = hidden_player(events, &manifest, [960, 540]);
			let press = |code| native_input::keyboard_key(PhysicalKey::Code(code)).unwrap();
			let progress = |app: &GameApp| *app.editor.play_progress::<CinderProgress>().unwrap();
			let step = |app: &mut GameApp, count| {
				for _ in 0..count {
					app.tick_play(1.0 / 60.0).unwrap();
					app.input.end_frame();
				}
			};
			let present = |app: &mut GameApp| {
				assert_eq!(app.render().unwrap(), renderer::DrawStatus::Rendered);
			};

			assert_eq!(progress(&app).phase(), Phase::Intro);
			let intro_eye = app.editor.play_rig_camera().unwrap().position;
			present(&mut app);
			step(&mut app, 150);
			assert_eq!(progress(&app).phase(), Phase::Intro);
			assert!(app.editor.play_rig_camera().unwrap().position[2] < intro_eye[2] - 1.0);
			present(&mut app);
			step(&mut app, 80);
			assert_eq!(app.editor.play_scene_id().unwrap().as_str(), "arena");
			assert_eq!(progress(&app).phase(), Phase::Duel);

			// The window is hidden so capture ownership is supplied without moving the
			// user's pointer. Mouse deltas and buttons still use the Player mapping.
			let start_eye = app.editor.play_rig_camera().unwrap();
			app.orbit_capture = app.editor.camera_input_owner();
			assert!(app.orbit_capture.is_some());
			app.mouse_captured = true;
			app.captured_motion(40.0, 0.0);
			step(&mut app, 1);
			assert!((app.editor.play_rig_camera().unwrap().yaw - start_eye.yaw).abs() > 0.02);
			app.captured_motion(-40.0, 0.0);
			step(&mut app, 1);
			app.input.press(press(KeyCode::KeyW));
			step(&mut app, 34);
			app.input.release(press(KeyCode::KeyW));
			assert!(
				app.editor.play_rig_camera().unwrap().position[2] < start_eye.position[2] - 1.0
			);
			present(&mut app);

			let left = native_input::mouse_button(MouseButton::Left);
			for attempt in 0..3 {
				if attempt == 0 {
					app.input.press(left);
				} else {
					app.input.press(press(KeyCode::KeyF));
				}
				step(&mut app, 1);
				app.input.release(left);
				app.input.release(press(KeyCode::KeyF));
				step(&mut app, 46);
				let run = progress(&app);
				assert_eq!(run.swings(), attempt + 1);
				assert_eq!(run.enemy_hits(), attempt + 1);
			}
			assert_eq!(progress(&app).phase(), Phase::Victory);
			assert!(app.game_completed());
			assert!(app.cinder_victory_animation_active());
			assert!(
				app.editor
					.play_status()
					.unwrap()
					.gameplay
					.unwrap()
					.completed
			);
			for _ in 0..90 {
				app.update_player(1.0 / 60.0);
			}
			assert!(!app.cinder_victory_animation_active());
			assert!(app.game_completed());
			present(&mut app);
			app.wait_for_gpu().unwrap();
			app.input.press(Key::F10);
			app.update_player(1.0 / 60.0);
			assert_eq!(progress(&app).phase(), Phase::Intro);
			assert_eq!(app.editor.play_scene_id().unwrap().as_str(), "intro");
			println!("Cinder Oath rendered the intro, entered the arena, and won in three hits");
		});
		return;
	}

	let directory = evidence_directory("cinder-oath");
	let output = Command::new(std::env::current_exe().unwrap())
		.args([
			"--ignored",
			"--exact",
			"camera_native_tests::cinder_oath_native_player_completes_intro_and_sword_duel",
			"--nocapture",
		])
		.env(CHILD, "1")
		.env("LOCALAPPDATA", &directory)
		.env("VK_INSTANCE_LAYERS", "VK_LAYER_LUNARG_screenshot")
		.env("VK_SCREENSHOT_DIR", &directory)
		.env("VK_SCREENSHOT_FRAMES", "0,1,2,3")
		.output()
		.unwrap();
	assert!(
		output.status.success(),
		"{}\n{}",
		String::from_utf8_lossy(&output.stdout),
		String::from_utf8_lossy(&output.stderr),
	);
	let captures = fs::read_dir(&directory)
		.unwrap()
		.map(|entry| entry.unwrap().path())
		.filter(|path| path.extension().is_some_and(|ext| ext == "ppm"))
		.collect::<Vec<_>>();
	assert_eq!(
		captures.len(),
		4,
		"Vulkan capture count in {}",
		directory.display()
	);
	for path in captures {
		let image = image::open(path).unwrap().to_rgb8();
		assert_eq!(image.dimensions(), (960, 540));
		let first = *image.get_pixel(0, 0);
		assert!(image.pixels().any(|pixel| *pixel != first));
	}
	print!("{}", String::from_utf8_lossy(&output.stdout));
	println!("Cinder Oath screenshots: {}", directory.display());
}

#[test]
#[ignore = "requires native Vulkan and the Vulkan SDK screenshot layer"]
fn game_content_rect_native_images() {
	const CHILD: &str = "GAME_ENGINE_QA_CAPTURE_WIDTH";
	if let Ok(width) = std::env::var(CHILD) {
		let width: u32 = width.parse().unwrap();
		with_native_window(|events| {
			let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../projects/cinder-oath/project.json");
			let mut app = hidden_player(events, &manifest, [width, 256]);
			if width == 1024 {
				app.cursor_position = Some([127.0, 20.0]);
				assert_eq!(app.cursor_geometry().unwrap(), None);
				app.cursor_position = Some([128.0, 20.0]);
				assert_eq!(
					app.cursor_geometry().unwrap(),
					Some(([0.0, 20.0], [768, 256]))
				);
				app.cursor_position = Some([896.0, 20.0]);
				assert_eq!(app.cursor_geometry().unwrap(), None);
			}
			assert_eq!(app.render().unwrap(), renderer::DrawStatus::Rendered);
			assert!(app.game_hud.as_ref().and_then(|hud| hud.object()).is_some());
			app.wait_for_gpu().unwrap();
		});
		return;
	}
	let captures = evidence_directory("images");
	let capture = |width: u32| {
		let dir = captures.join(format!("{width}x256"));
		fs::create_dir(&dir).unwrap();
		let result = Command::new(std::env::current_exe().unwrap())
			.args([
				"--ignored",
				"--exact",
				"camera_native_tests::game_content_rect_native_images",
				"--nocapture",
			])
			.env(CHILD, width.to_string())
			.env("VK_INSTANCE_LAYERS", "VK_LAYER_LUNARG_screenshot")
			.env("VK_SCREENSHOT_DIR", &dir)
			.env("VK_SCREENSHOT_FRAMES", "0")
			.output()
			.unwrap();
		assert!(
			result.status.success(),
			"{}",
			String::from_utf8_lossy(&result.stderr)
		);
		let file = fs::read_dir(&dir)
			.unwrap()
			.map(|f| f.unwrap().path())
			.find(|p| p.extension().is_some_and(|e| e == "ppm"))
			.unwrap_or_else(|| {
				panic!(
					"Missing Vulkan screenshot in {}: {}",
					dir.display(),
					String::from_utf8_lossy(&result.stdout)
				)
			});
		let image = image::open(file).unwrap().to_rgb8();
		assert_eq!(image.dimensions(), (width, 256));
		image
	};
	let reference = capture(768_u32);
	let wide = capture(1024_u32);
	let portrait = capture(192_u32);
	let mut differing = 0;
	for (x, y, pixel) in wide.enumerate_pixels() {
		if (128..896).contains(&x) {
			let expected = reference.get_pixel(x - 128, y);
			differing += usize::from(
				pixel
					.0
					.iter()
					.zip(expected.0)
					.any(|(a, b)| a.abs_diff(b) > 1),
			);
		} else {
			assert_eq!(pixel.0, [0; 3], "Side bar pixel at ({x}, {y})");
		}
	}
	// At most a few raster-edge pixels may differ between translated viewports.
	assert!(differing < 150, "{differing} content pixels changed");
	let background = *reference.get_pixel(0, 0);
	assert_ne!(background.0, [0; 3]);
	assert!(
		reference
			.enumerate_pixels()
			.any(|(_, y, p)| y > 128 && *p != background),
		"Expected world geometry below the HUD"
	);
	assert!(
		reference.get_pixel(20, reference.height() - 24) != &background,
		"HUD must render near the content's left edge"
	);
	assert_eq!(portrait.get_pixel(0, 0), &background);
	assert_eq!(portrait.get_pixel(191, 0), &background);
	println!(
		"Compared {} reference pixels, {differing} differences above one channel level. Images: {}",
		768 * 256,
		captures.display()
	);
}

#[test]
#[ignore = "requires native Vulkan and the Vulkan SDK screenshot layer"]
fn accessible_player_overlay_renders_with_a_persisted_high_contrast_profile() {
	const CHILD: &str = "GAME_ENGINE_QA_ACCESSIBLE_CAPTURE";
	if std::env::var_os(CHILD).is_some() {
		with_native_window(|events| {
			let manifest =
				Path::new(env!("CARGO_MANIFEST_DIR")).join("../../projects/cinder-oath/project.json");
			let mut app = hidden_player(events, &manifest, [768, 256]);
			app.feedback_audio = feedback_audio::FeedbackAudio::default();
			app.player_controls
				.configure(Some(&manifest), None, false);
			for key in [
				Key::Escape,
				Key::ArrowUp,
				Key::ArrowUp,
				Key::Enter,
				Key::ArrowLeft,
				Key::ArrowDown,
				Key::ArrowRight,
				Key::ArrowDown,
				Key::ArrowDown,
				Key::Enter,
				Key::Escape,
			] {
				assert!(app.player_controls_key(key));
			}
			assert!(!app.player_controls.is_open());
			let expected = app.player_controls.display().overlay_style();
			assert_eq!(expected.text_scale, 1.5);
			assert!(expected.high_contrast);
			let mut relaunched = player_controls::PlayerControls::default();
			relaunched.configure(Some(&manifest), None, false);
			assert_eq!(relaunched.display().overlay_style(), expected);

			assert_eq!(app.editor.play_scene_id().unwrap().as_str(), "intro");
			assert_eq!(app.render().unwrap(), renderer::DrawStatus::Rendered);
			assert!(app.game_hud.as_ref().and_then(|hud| hud.object()).is_some());
			app.wait_for_gpu().unwrap();
		});
		return;
	}

	let directory = evidence_directory("accessibility");
	let output = Command::new(std::env::current_exe().unwrap())
		.args([
			"--ignored",
			"--exact",
			"camera_native_tests::accessible_player_overlay_renders_with_a_persisted_high_contrast_profile",
			"--nocapture",
		])
		.env(CHILD, "1")
		.env("LOCALAPPDATA", &directory)
		.env("VK_INSTANCE_LAYERS", "VK_LAYER_LUNARG_screenshot")
		.env("VK_SCREENSHOT_DIR", &directory)
		.env("VK_SCREENSHOT_FRAMES", "0")
		.output()
		.unwrap();
	assert!(
		output.status.success(),
		"{}\n{}",
		String::from_utf8_lossy(&output.stdout),
		String::from_utf8_lossy(&output.stderr),
	);
	let image = fs::read_dir(&directory)
		.unwrap()
		.map(|entry| entry.unwrap().path())
		.find(|path| path.extension().is_some_and(|ext| ext == "ppm"))
		.expect("Vulkan screenshot");
	let pixels = image::open(&image).unwrap().to_rgb8();
	let panel = pixels.pixels().any(|p| p.0 == [0; 3]);
	let glyphs = pixels.pixels().any(|p| p.0 == [255; 3]);
	assert!(panel && glyphs, "expected black panel and white glyphs");
	println!("Large high-contrast Player screenshot: {}", image.display());
}

#[path = "../../../../tests/support/project_fixture.rs"]
mod test_project;
