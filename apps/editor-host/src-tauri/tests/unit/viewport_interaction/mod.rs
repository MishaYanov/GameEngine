//! CPU acceptance tests across the native interaction controller and real editor
//! transactions. Rendering bounds are cached fixtures; no GPU is required.
use super::*;
use bridge::editor::ViewportModifiersDto;
use runtime::{
	scene_data::{CameraData, EntityData, SceneData, TransformData},
	EntityKey,
};
use std::{
	fs,
	path::PathBuf,
	sync::atomic::{AtomicU64, Ordering},
};

const EXTENT: [u32; 2] = [800, 600];

#[test]
fn camera_room_preview_restores_scene_and_applied_camera_survives_undo_save_and_play() {
	let temp = Fixture::new(0);
	let demo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game");
	let project = runtime::project::ProjectData::load(demo.join("camera-demo.json")).unwrap();
	let prepared = compiler::prepare_project(&project, &demo.join("assets")).unwrap();
	let staged =
		compiler::stage_prepared_project(&prepared, temp.directory.to_str().unwrap()).unwrap();
	let open = || {
		let location = crate::project_location::ProjectLocation::from_manifest(
			&staged.path().join("project.json"),
		)
		.unwrap();
		EditorWorkspace::open_project_with_notifier(location, || {}).unwrap()
	};
	let mut workspace = open();
	let mut view = controller(&mut workspace);
	select(&mut workspace, &["player"]);
	view.sync(&mut workspace).unwrap();
	let before = view.state();
	let original = workspace.backend.session().authored_document();
	let revision = workspace.backend.session().revision();
	let bytes = fs::read(&workspace.scene_path).unwrap();
	let mut candidate = original.camera.clone();
	candidate.orbit.as_mut().unwrap().initial_distance = 5.5;
	send(
		&mut view,
		&mut workspace,
		event("pointer_down", [30.0; 2], Some("right")),
	);
	assert!(view.captured());
	let preview = workspace
		.open_camera_preview(
			candidate.clone(),
			revision,
			workspace.generation,
			"camera-room",
		)
		.unwrap();
	view.sync(&mut workspace).unwrap();
	let initial = view.game_camera(&workspace).unwrap();
	assert!(!view.captured());
	assert_eq!(
		initial.position,
		workspace.active_camera_preview().unwrap().position
	);
	assert_eq!(
		view.visuals(&workspace, EXTENT, 1.0).unwrap(),
		EditorVisuals::default()
	);
	assert!(view
		.action(
			ViewportActionDto::SetGrid { visible: false },
			&mut workspace,
			EXTENT
		)
		.is_err());
	send(
		&mut view,
		&mut workspace,
		event("pointer_down", [30.0; 2], Some("left")),
	);
	send(&mut view, &mut workspace, key("Delete"));
	assert!(!view.captured());
	workspace
		.sample_camera_preview(
			&preview.token,
			1,
			CameraIntent {
				yaw_delta: 0.25,
				..Default::default()
			},
		)
		.unwrap();
	assert_ne!(
		view.game_camera(&workspace).unwrap().position,
		initial.position
	);
	assert!(workspace.close_camera_preview(&preview.token));
	view.sync(&mut workspace).unwrap();
	assert!(view.game_camera(&workspace).is_none());
	assert_eq!(view.state().camera_position, before.camera_position);
	assert_eq!(view.state().camera_target, before.camera_target);
	assert_eq!(view.state().settings, before.settings);
	assert_eq!(selection(&workspace), ["player"]);
	assert_eq!(workspace.backend.session().revision(), revision);
	assert!(workspace.backend.camera_replay_state().unwrap().is_none());
	assert_eq!(
		workspace
			.backend
			.session()
			.authored_document()
			.to_json()
			.unwrap(),
		original.to_json().unwrap()
	);
	assert_eq!(fs::read(&workspace.scene_path).unwrap(), bytes);

	command(
		&mut workspace,
		Command::SetCamera {
			camera: candidate.clone(),
		},
	);
	command(&mut workspace, Command::Undo);
	assert_eq!(
		workspace.backend.session().authored_document().camera,
		original.camera
	);
	command(&mut workspace, Command::Redo);
	workspace
		.save_all(workspace.backend.session().revision())
		.unwrap();
	drop(workspace);
	let mut workspace = open();
	assert_eq!(
		workspace.backend.session().authored_document().camera,
		candidate
	);
	command(&mut workspace, Command::StartPlay);
	view.action(
		ViewportActionDto::SetView {
			view: ViewportView::Game,
		},
		&mut workspace,
		EXTENT,
	)
	.unwrap();
	assert_eq!(
		view.game_camera(&workspace).unwrap().position,
		initial.position
	);
	let advance = |w: &mut EditorWorkspace, v: &mut ViewportInteraction, ticks| {
		for _ in 0..ticks {
			let tick = w.backend.tick(1.0 / 60.0);
			v.sync_game_status_after_tick(w, &tick);
			tick.unwrap();
		}
	};
	let movement = |w: &mut EditorWorkspace, movement_xz| {
		command(
			w,
			Command::SetCharacterInput {
				key: "player".into(),
				movement_xz,
				jump_held: false,
			},
		)
	};
	movement(&mut workspace, [0.0, 1.0]);
	advance(&mut workspace, &mut view, 60);
	movement(&mut workspace, [0.0; 2]);
	advance(&mut workspace, &mut view, 180);
	assert_eq!(
		view.state().orbit_status.unwrap().blocker.as_deref(),
		Some("camera-wall")
	);
	movement(&mut workspace, [1.0, 0.0]);
	advance(&mut workspace, &mut view, 60);
	movement(&mut workspace, [0.0; 2]);
	advance(&mut workspace, &mut view, 240);
	let clear = view.state().orbit_status.unwrap();
	assert!(clear.blocker.is_none() && !clear.target_lost && !clear.recovering);
	let rig = workspace.backend.session().orbit_state().unwrap();
	assert_eq!(rig.effective_distance, 5.5);
	command(&mut workspace, Command::SetPaused { paused: true });
	assert_eq!(
		view.game_camera(&workspace).unwrap().position,
		workspace.backend.session().render_camera().position
	);
	command(&mut workspace, Command::StopPlay);
	view.action(
		ViewportActionDto::SetView {
			view: ViewportView::Scene,
		},
		&mut workspace,
		EXTENT,
	)
	.unwrap();
	assert!(view.state().orbit_status.is_none());
	assert!(view.game_camera(&workspace).is_none());
	assert_eq!(
		workspace.backend.session().authored_document().camera,
		candidate
	);
}

fn collider_overlay(controller: &ViewportInteraction) -> EditorVisuals {
	let mut visuals = EditorVisuals::default();
	controller.collider_visuals.append(&mut visuals, 1.0);
	visuals.validate().unwrap();
	visuals
}

struct Fixture {
	directory: PathBuf,
	assets: PathBuf,
}
impl Fixture {
	fn new(count: usize) -> Self {
		static NEXT: AtomicU64 = AtomicU64::new(0);
		let target = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
			.join("../../../target")
			.canonicalize()
			.unwrap();
		let directory = target.join(format!(
			"qa-viewport-interaction-{}-{}",
			std::process::id(),
			NEXT.fetch_add(1, Ordering::Relaxed)
		));
		fs::create_dir(&directory).unwrap();
		let assets = directory.join("assets");
		fs::create_dir(&assets).unwrap();
		fs::create_dir(assets.join("scenes")).unwrap();
		fs::create_dir(assets.join("models")).unwrap();
		write_triangle_glb(&assets.join("models/triangle.glb"));
		let camera = runtime::Camera::default();
		let document = SceneData {
			schema_version: runtime::scene_data::CURRENT_SCHEMA_VERSION,
			bindings: Default::default(),
			entities: (0..count)
				.map(|index| {
					EntityData::new(
						EntityKey::new(format!("entity-{index}")).unwrap(),
						format!("Entity {index}"),
						"models/triangle.glb",
					)
					.with_transform(TransformData {
						position: [index as f32 * 3.0, 0.0, 0.0],
						..Default::default()
					})
				})
				.collect(),
			camera: CameraData {
				follow: None,
				orbit: None,
				position: camera.position,
				yaw: camera.yaw,
				pitch: camera.pitch,
				vertical_fov: camera.vertical_fov,
				near_plane: camera.near,
				far_plane: camera.far,
			},
			lights: vec![],
		};
		fs::write(assets.join("scenes/main.json"), document.to_json().unwrap()).unwrap();
		Self { directory, assets }
	}
	fn open(&self) -> EditorWorkspace {
		EditorWorkspace::open(self.assets.to_str().unwrap(), "scenes/main.json").unwrap()
	}
}
impl Drop for Fixture {
	fn drop(&mut self) {
		let target = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
			.join("../../../target")
			.canonicalize()
			.unwrap();
		assert_eq!(self.directory.parent(), Some(target.as_path()));
		assert!(self
			.directory
			.file_name()
			.unwrap()
			.to_str()
			.unwrap()
			.starts_with("qa-viewport-interaction-"));
		let _ = fs::remove_dir_all(&self.directory);
	}
}

fn command(workspace: &mut EditorWorkspace, command: Command) {
	workspace
		.execute_receipt(EditorCommandRequest {
			expected_revision: workspace.backend.session().revision(),
			command,
		})
		.unwrap();
}

fn controller(workspace: &mut EditorWorkspace) -> ViewportInteraction {
	let mut controller = ViewportInteraction::default();
	controller.sync(workspace).unwrap();
	controller.geometry_changed(workspace, EXTENT, 1.0).unwrap();
	controller.camera.pivot = Vec3::ZERO;
	controller.camera.yaw = 0.0;
	controller.camera.pitch = 0.0;
	controller.camera.distance = 10.0;
	controller.camera.ortho_height = 10.0;
	controller.settings.projection = ViewportProjection::Orthographic;
	cache_targets(&mut controller, workspace);
	controller
}

fn cache_targets(controller: &mut ViewportInteraction, workspace: &EditorWorkspace) {
	controller.targets.clear();
	controller.target_indices.clear();
	let document = workspace.backend.session().authored_document();
	let worlds = document.world_matrices().unwrap();
	for (entity, world) in document.entities.iter().zip(worlds) {
		controller
			.target_indices
			.insert(entity.key.as_str().to_owned(), controller.targets.len());
		controller.targets.push(PickTarget {
			key: entity.key.as_str().to_owned(),
			bounds: Bounds3::new([-0.5; 3], [0.5; 3])
				.unwrap()
				.transformed(world)
				.unwrap(),
		});
	}
	controller.initial_frame = false;
	controller.target_revision = Some((
		workspace.generation,
		workspace.backend.session().scene_revision(),
	));
}

fn write_triangle_glb(path: &std::path::Path) {
	let mut json = br#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"name":"Triangle","mesh":0}],"meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],"buffers":[{"byteLength":36}],"bufferViews":[{"buffer":0,"byteLength":36,"target":34962}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[-0.5,0.0,0.0],"max":[0.5,1.0,0.0]}]}"#.to_vec();
	while !json.len().is_multiple_of(4) {
		json.push(b' ');
	}
	let vertices: [f32; 9] = [-0.5, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 1.0, 0.0];
	let mut bytes = Vec::new();
	for word in [
		0x46546C67_u32,
		2,
		(12 + 8 + json.len() + 8 + 36) as u32,
		json.len() as u32,
		0x4E4F534A,
	] {
		bytes.extend_from_slice(&word.to_le_bytes());
	}
	bytes.extend(json);
	bytes.extend_from_slice(&36_u32.to_le_bytes());
	bytes.extend_from_slice(&0x004E4942_u32.to_le_bytes());
	for value in vertices {
		bytes.extend_from_slice(&value.to_le_bytes());
	}
	fs::write(path, bytes).unwrap();
}

fn begin_valid_placement(controller: &mut ViewportInteraction, workspace: &mut EditorWorkspace) {
	controller
		.action(
			ViewportActionDto::Orient {
				orientation: ViewportOrientation::Top,
			},
			workspace,
			EXTENT,
		)
		.unwrap();
	controller
		.action(
			ViewportActionDto::BeginPlacement {
				path: "models/triangle.glb".into(),
			},
			workspace,
			EXTENT,
		)
		.unwrap();
	controller
		.action(
			ViewportActionDto::UpdatePlacement {
				normalized: Some([0.5, 0.5]),
			},
			workspace,
			EXTENT,
		)
		.unwrap();
	assert!(controller.state().placement.unwrap().position.is_some());
}

fn drain_until_complete(
	controller: &mut ViewportInteraction,
	workspace: &mut EditorWorkspace,
	job: runtime::asset_jobs::AssetJobId,
) {
	let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
	loop {
		controller.drain_imports(workspace);
		if workspace
			.recent_completions()
			.iter()
			.any(|completion| completion.job_id == job)
		{
			return;
		}
		assert!(
			std::time::Instant::now() < deadline,
			"Small fixture import did not complete"
		);
		std::thread::sleep(std::time::Duration::from_millis(1));
	}
}

fn event(kind: &str, point: [f64; 2], button: Option<&str>) -> ViewportInputDto {
	ViewportInputDto {
		kind: kind.into(),
		position: Some(point),
		extent: EXTENT,
		button: button.map(str::to_owned),
		code: None,
		repeat: false,
		modifiers: ViewportModifiersDto::default(),
		delta: None,
	}
}
fn send(
	controller: &mut ViewportInteraction,
	workspace: &mut EditorWorkspace,
	event: ViewportInputDto,
) {
	controller.input(&event, workspace, EXTENT, 1.0).unwrap();
}
fn key(code: &str) -> ViewportInputDto {
	ViewportInputDto {
		code: Some(code.into()),
		..event("key_down", [0.0; 2], None)
	}
}
fn focus_keyboard(controller: &mut ViewportInteraction, workspace: &mut EditorWorkspace) {
	send(controller, workspace, event("focused", [0.0; 2], None));
}

fn start_walk(controller: &mut ViewportInteraction, workspace: &mut EditorWorkspace) {
	let mut walk = key("Backquote");
	walk.modifiers.shift = true;
	send(controller, workspace, walk);
	assert_eq!(controller.state().gesture.as_deref(), Some("fly"));
}
fn select(workspace: &mut EditorWorkspace, keys: &[&str]) {
	command(
		workspace,
		Command::Select {
			keys: keys.iter().map(|key| (*key).into()).collect(),
		},
	);
}
fn selection(workspace: &EditorWorkspace) -> Vec<String> {
	workspace
		.backend
		.session()
		.selection()
		.iter()
		.map(|key| key.as_str().to_owned())
		.collect()
}
fn pixel(controller: &ViewportInteraction, world: Vec3) -> [f64; 2] {
	let point = controller
		.camera
		.project(world, EXTENT, controller.settings.projection)
		.unwrap();
	[point.x as f64, point.y as f64]
}
fn click(
	controller: &mut ViewportInteraction,
	workspace: &mut EditorWorkspace,
	point: [f64; 2],
	shift: bool,
	control: bool,
) {
	let mut down = event("pointer_down", point, Some("left"));
	down.modifiers.shift = shift;
	down.modifiers.control = control;
	send(controller, workspace, down);
	send(
		controller,
		workspace,
		event("pointer_up", point, Some("left")),
	);
}
fn axis_drag_start(
	controller: &mut ViewportInteraction,
	workspace: &mut EditorWorkspace,
) -> [f64; 2] {
	select(workspace, &["entity-0"]);
	let gizmo = controller.gizmo(workspace, EXTENT, 1.0).unwrap().unwrap();
	let point = pixel(controller, gizmo.pivot + gizmo.axes[0] * gizmo.size * 0.7);
	send(
		controller,
		workspace,
		event("pointer_down", point, Some("left")),
	);
	assert!(
		controller.drag.is_some(),
		"A visible Move X handle should capture a transform drag"
	);
	point
}

#[test]
fn project_replacement_drops_view_gestures_and_bookmarks_without_changing_new_document() {
	let first = Fixture::new(1);
	let second = Fixture::new(1);
	let mut workspace = first.open();
	let mut controller = controller(&mut workspace);
	controller
		.action(
			ViewportActionDto::BeginPlacement {
				path: "models/triangle.glb".into(),
			},
			&mut workspace,
			EXTENT,
		)
		.unwrap();
	controller.camera.pivot = Vec3::new(10.0, 11.0, 12.0);
	let mut replacement = second.open();
	let before = serde_json::to_value(replacement.backend.snapshot().unwrap()).unwrap();
	controller.sync(&mut replacement).unwrap();
	assert!(controller.state().placement.is_none());
	assert!(!controller.captured());
	assert_eq!(
		controller.state().workspace_generation,
		Some(replacement.generation)
	);
	assert_eq!(controller.camera.pivot, EditorCamera::default().pivot);
	assert_eq!(
		serde_json::to_value(replacement.backend.snapshot().unwrap()).unwrap(),
		before
	);
}

#[test]
fn a_stale_gesture_cannot_cancel_an_identically_labelled_transaction_in_another_project() {
	let first = Fixture::new(1);
	let second = Fixture::new(1);
	let mut workspace = first.open();
	let mut controller = controller(&mut workspace);
	axis_drag_start(&mut controller, &mut workspace);
	let mut replacement = second.open();
	command(
		&mut replacement,
		Command::BeginTransaction {
			label: TRANSFORM_LABEL.into(),
		},
	);
	let before = serde_json::to_value(replacement.backend.snapshot().unwrap()).unwrap();
	controller.cancel(&mut replacement).unwrap();
	assert_eq!(
		serde_json::to_value(replacement.backend.snapshot().unwrap()).unwrap(),
		before
	);
	assert!(replacement.backend.session().has_transaction());
	command(&mut replacement, Command::CancelTransaction);
	command(&mut workspace, Command::CancelTransaction);
}

mod colliders;
mod gestures;
mod navigation;
mod play;
