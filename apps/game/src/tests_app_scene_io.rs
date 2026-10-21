use super::*;
use runtime::{
    SceneData, Transform, assets::Model, model_instance::ModelInstance, scene::EntityId,
};
use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        // Workspace target permits rename; the Windows sandbox's Temp does not.
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .join(format!(
                "game-engine-app-scene-{}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir(&directory).unwrap();
        Self(directory)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture() -> (GameApp, [EntityId; 2]) {
    let model = Arc::new(Model::new(Vec::new()));
    let mut app = GameApp::default();
    let ids = [-0.8, 0.8].map(|x| {
        app.scene.spawn_model(
            format!("Robot at {x}"),
            "props/robot.glb",
            ModelInstance::new(
                Arc::clone(&model),
                Transform {
                    position: [x, 0.0, 0.0],
                    scale: [0.7; 3],
                    ..Transform::default()
                },
            ),
        )
    });
    app.game = Some(RobotGame::from_entities(&ids).unwrap());
    app.input.press(Key::W);
    app.input.press(Key::ArrowRight);
    (app, ids)
}

fn snapshot(app: &GameApp) -> String {
    app.scene.to_data(&app.camera).unwrap().to_json().unwrap()
}

fn prime_clock(app: &mut GameApp) -> Instant {
    let start = Instant::now();
    assert_eq!(app.clock.tick(start), Duration::ZERO);
    start
}

#[test]
fn helpers_report_missing_configuration_without_mutating_live_state() {
    let directory = TestDirectory::new();
    let (mut app, _) = fixture();
    let before = snapshot(&app);
    assert!(app.save_scene().unwrap_err().contains("Scene path"));
    assert!(app.reload_scene().unwrap_err().contains("Scene path"));
    app.scene_path = Some(directory.0.join("robots.json"));
    assert!(app.reload_scene().unwrap_err().contains("Asset root"));
    app.asset_root = Some(directory.0.clone());
    assert!(app.reload_scene().unwrap_err().contains("Renderer"));
    assert_eq!(snapshot(&app), before);
    assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 0);
}

#[test]
fn f5_saves_before_motion_resets_clock_and_requires_a_fresh_press_to_save_again() {
    let directory = TestDirectory::new();
    let path = directory.0.join("robots.json");
    let (mut app, [left, _]) = fixture();
    app.scene_path = Some(path.clone());
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
fn installed_project_guard_prevents_save_reload_motion_and_clock_reset() {
    let directory = TestDirectory::new();
    let path = directory.0.join("robots.json");
    fs::write(&path, "installed project scene").unwrap();
    let (mut app, _) = fixture();
    app.scene_path = Some(path.clone());
    app.asset_root = Some(directory.0.clone());
    app.project_name = Some("Installed project".to_owned());
    let before = snapshot(&app);
    let start = prime_clock(&mut app);
    app.input.press(Key::F5);
    app.input.press(Key::F9);
    app.update(0.5);
    assert_eq!(snapshot(&app), before);
    assert_eq!(fs::read_to_string(path).unwrap(), "installed project scene");
    assert_eq!(
        app.clock.tick(start + Duration::from_millis(16)),
        Duration::from_millis(16)
    );
}
