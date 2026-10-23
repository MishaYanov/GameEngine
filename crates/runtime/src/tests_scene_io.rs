use super::save;
use crate::{
	Camera, EntityKey, SceneData, Transform,
    assets::Model,
    model_instance::ModelInstance,
    scene::{EntityId, Scene},
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        // Workspace target permits rename; the Windows sandbox's Temp does not.
        let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .join(format!(
                "game-engine-scene-io-{}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir(&directory).unwrap();
        Self(directory)
    }

    fn only_destination_remains(&self) {
        let entries: Vec<_> = fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(entries, vec![std::ffi::OsString::from("robots.json")]);
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn instance(model: &Arc<Model>, x: f32) -> ModelInstance {
    ModelInstance::new(
        Arc::clone(model),
        Transform {
            position: [x, 0.0, 0.0],
            rotation: [0.1, 0.2, 0.3],
            scale: [0.7; 3],
        },
    )
}

fn fixture() -> (Scene, Camera, [EntityId; 2]) {
    let model = Arc::new(Model::new(Vec::new()));
    let mut scene = Scene::default();
	let left = scene
		.spawn_keyed_model(
			EntityKey::new("left").unwrap(),
			"Left label",
			"props/robot.glb",
			instance(&model, -0.8),
		)
		.unwrap();
	let right = scene
		.spawn_keyed_model(
			EntityKey::new("right").unwrap(),
			"Right label",
			"props/alternate.gltf",
			instance(&model, 0.8),
		)
		.unwrap();
	scene.set_binding("robot_a", left).unwrap();
	scene.set_binding("robot_b", right).unwrap();
    (scene, Camera::default(), [left, right])
}

fn assert_saved_matches(path: &Path, scene: &Scene, camera: &Camera) {
    let source = fs::read_to_string(path).unwrap();
    let saved = SceneData::from_json(&source).unwrap();
    assert_eq!(
        serde_json::to_value(saved).unwrap(),
        serde_json::to_value(scene.to_data(camera).unwrap()).unwrap(),
    );
}

#[test]
fn snapshot_keeps_names_paths_order_live_transforms_and_all_camera_settings() {
    let (mut scene, mut camera, [left, right]) = fixture();
    let removed = scene.spawn_model(
        "Removed",
        "props/unused.glb",
        instance(&Arc::new(Model::new(Vec::new())), 7.0),
    );
    scene.remove(removed).unwrap();
    assert_eq!(scene.name(removed), None);
    let transform = scene.get_mut(left).unwrap().transform_mut();
    transform.position = [2.0, 3.0, 4.0];
    transform.rotation = [-0.2, 0.4, 0.8];
    transform.scale = [1.0, 2.0, 3.0];
    camera.position = [5.0, 6.0, 7.0];
    camera.yaw = 0.3;
    camera.pitch = -0.2;
    camera.vertical_fov = 0.9;
    camera.near = 0.2;
    camera.far = 250.0;

    let data = scene.to_data(&camera).unwrap();
	assert_eq!(
		data.schema_version,
		crate::scene_data::CURRENT_SCHEMA_VERSION
	);
	assert_eq!(data.bindings["robot_a"].as_str(), "left");
	assert_eq!(data.bindings["robot_b"].as_str(), "right");
    assert_eq!(data.entities.len(), 2);
    assert_eq!(data.entities[0].name, "Left label");
    assert_eq!(data.entities[0].model_path, "props/robot.glb");
    assert_eq!(data.entities[0].transform.position, [2.0, 3.0, 4.0]);
    assert_eq!(data.entities[0].transform.rotation, [-0.2, 0.4, 0.8]);
    assert_eq!(data.entities[0].transform.scale, [1.0, 2.0, 3.0]);
    assert_eq!(data.entities[1].name, "Right label");
    assert_eq!(data.entities[1].model_path, "props/alternate.gltf");
    assert_eq!(data.entities[1].transform.position, [0.8, 0.0, 0.0]);
    assert_eq!(data.camera.position, camera.position);
    assert_eq!(
        (data.camera.yaw, data.camera.pitch),
        (camera.yaw, camera.pitch)
    );
    assert_eq!(
        (
            data.camera.vertical_fov,
            data.camera.near_plane,
            data.camera.far_plane
        ),
        (camera.vertical_fov, camera.near, camera.far)
    );
    assert_eq!(scene.name(left), Some("Left label"));
    assert_eq!(scene.name(right), Some("Right label"));
}

#[test]
fn pathless_spawns_keep_names_but_report_that_they_cannot_be_snapshotted() {
    let model = Arc::new(Model::new(Vec::new()));
    let mut scene = Scene::default();
    let unnamed = scene.spawn(instance(&model, 0.0));
    assert_eq!(scene.name(unnamed), Some("entity"));
    let error = scene.to_data(&Camera::default()).unwrap_err();
    assert!(error.contains("model path"), "{error}");
    scene.remove(unnamed).unwrap();
    assert_eq!(scene.name(unnamed), None);
    let named = scene.spawn_named("Procedural robot", instance(&model, 0.0));
    let error = scene.to_data(&Camera::default()).unwrap_err();
    assert!(error.contains("Procedural robot"), "{error}");
    assert!(error.contains("model path"), "{error}");
    assert_eq!(scene.name(named), Some("Procedural robot"));
}

#[test]
fn save_replaces_existing_destination_with_current_scene_and_camera() {
    let directory = TestDirectory::new();
    let path = directory.0.join("robots.json");
    fs::write(&path, "old destination").unwrap();
    let (mut scene, mut camera, [left, _]) = fixture();
    save(&path, &scene, &camera).unwrap();
    assert_saved_matches(&path, &scene, &camera);

    scene.get_mut(left).unwrap().transform_mut().position = [9.0, 8.0, 7.0];
    camera.position = [6.0, 5.0, 4.0];
    camera.yaw = 0.4;
    save(&path, &scene, &camera).unwrap();
    assert_saved_matches(&path, &scene, &camera);
    directory.only_destination_remains();
}

#[test]
fn invalid_live_state_preserves_previous_destination_without_temporary_files() {
    let directory = TestDirectory::new();
    let path = directory.0.join("robots.json");
    let (mut scene, mut camera, [left, _]) = fixture();
    save(&path, &scene, &camera).unwrap();
    let previous = fs::read(&path).unwrap();

    camera.near = 0.0;
    assert!(save(&path, &scene, &camera).is_err());
    assert_eq!(fs::read(&path).unwrap(), previous);
    camera.near = 0.1;
    scene.get_mut(left).unwrap().transform_mut().position[1] = f32::NAN;
    assert!(save(&path, &scene, &camera).is_err());
    assert_eq!(fs::read(&path).unwrap(), previous);
    directory.only_destination_remains();
}

#[test]
fn failed_replacement_keeps_destination_contents_and_removes_temporary_file() {
    let directory = TestDirectory::new();
    let path = directory.0.join("robots.json");
    fs::create_dir(&path).unwrap();
    let marker = path.join("keep.txt");
    fs::write(&marker, "original contents").unwrap();
    let (scene, camera, _) = fixture();
    assert!(save(&path, &scene, &camera).is_err());
    assert_eq!(fs::read_to_string(marker).unwrap(), "original contents");
    directory.only_destination_remains();
}

#[cfg(windows)]
#[test]
fn locked_destination_file_survives_failed_windows_replacement() {
    use std::os::windows::fs::OpenOptionsExt;

    let directory = TestDirectory::new();
    let path = directory.0.join("robots.json");
    fs::write(&path, "original scene bytes").unwrap();
    let locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&path)
        .unwrap();
    let (scene, camera, _) = fixture();
    assert!(save(&path, &scene, &camera).is_err());
    drop(locked);
    assert_eq!(fs::read_to_string(&path).unwrap(), "original scene bytes");
    directory.only_destination_remains();
}

#[test]
fn missing_destination_parent_returns_an_error_without_creating_directories() {
    let directory = TestDirectory::new();
    let path = directory.0.join("missing").join("robots.json");
    let (scene, camera, _) = fixture();
    assert!(save(&path, &scene, &camera).is_err());
    assert!(!path.parent().unwrap().exists());
    assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 0);
}

#[test]
fn pathless_or_invalid_model_metadata_cannot_overwrite_a_saved_scene() {
    let directory = TestDirectory::new();
    let path = directory.0.join("robots.json");
    let (mut scene, camera, _) = fixture();
    save(&path, &scene, &camera).unwrap();
    let previous = fs::read(&path).unwrap();
    let model = Arc::new(Model::new(Vec::new()));
    let pathless = scene.spawn_named("Procedural", instance(&model, 1.0));
    assert!(
        save(&path, &scene, &camera)
            .unwrap_err()
            .contains("source model path")
    );
    assert_eq!(fs::read(&path).unwrap(), previous);
    scene.remove(pathless).unwrap();
    scene.spawn_model("Invalid path", "../outside.glb", instance(&model, 1.0));
    assert!(save(&path, &scene, &camera).is_err());
    assert_eq!(fs::read(&path).unwrap(), previous);
    directory.only_destination_remains();
}
