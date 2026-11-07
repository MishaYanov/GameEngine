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
	let mut json=br#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"name":"Triangle","mesh":0}],"meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],"buffers":[{"byteLength":36}],"bufferViews":[{"buffer":0,"byteLength":36,"target":34962}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[-0.5,0.0,0.0],"max":[0.5,1.0,0.0]}]}"#.to_vec();
	while json.len() % 4 != 0 {
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
fn navigation_and_view_preferences_never_change_authored_camera_revision_or_history() {
	let fixture = Fixture::new(2);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	select(&mut workspace, &["entity-0"]);
	let original = serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap();
	let original_position = controller.camera.position();
	for action in [
		ViewportActionDto::Zoom { factor: 0.8 },
		ViewportActionDto::FrameSelection,
		ViewportActionDto::Orient {
			orientation: ViewportOrientation::Top,
		},
		ViewportActionDto::SetGrid { visible: false },
		ViewportActionDto::SetGridSpacing { spacing: 0.25 },
		ViewportActionDto::SetSnap { enabled: true },
		ViewportActionDto::SetPlane {
			plane: ViewportPlane::Xy,
		},
		ViewportActionDto::SetSpace {
			space: ViewportSpace::Local,
		},
		ViewportActionDto::SetPivot {
			pivot: ViewportPivot::Center,
		},
	] {
		controller.action(action, &mut workspace, EXTENT).unwrap();
	}
	let mut down = event("pointer_down", [200.0, 200.0], Some("left"));
	down.modifiers.alt = true;
	send(&mut controller, &mut workspace, down);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_move", [240.0, 220.0], None),
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_up", [240.0, 220.0], Some("left")),
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_down", [200.0, 200.0], Some("middle")),
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_move", [220.0, 250.0], None),
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_up", [220.0, 250.0], Some("middle")),
	);
	let mut wheel = event("wheel_lines", [200.0, 200.0], None);
	wheel.delta = Some([0.0, 2.0]);
	send(&mut controller, &mut workspace, wheel);
	assert_ne!(controller.camera.position(), original_position);
	assert_eq!(
		serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap(),
		original
	);
	assert!(!workspace.backend.session().is_dirty());
	assert!(!workspace.backend.session().state().can_undo);
}

#[test]
fn clicking_shift_adds_control_toggles_and_empty_click_clears_selection() {
	let fixture = Fixture::new(2);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	let first = pixel(&controller, Vec3::new(-0.3, 0.0, 0.0));
	let second = pixel(&controller, Vec3::new(3.0, 0.0, 0.0));
	click(&mut controller, &mut workspace, first, false, false);
	assert_eq!(selection(&workspace), ["entity-0"]);
	click(&mut controller, &mut workspace, second, true, false);
	assert_eq!(selection(&workspace), ["entity-0", "entity-1"]);
	click(&mut controller, &mut workspace, first, false, true);
	assert_eq!(selection(&workspace), ["entity-1"]);
	click(&mut controller, &mut workspace, [10.0, 10.0], false, false);
	assert!(selection(&workspace).is_empty());
	assert!(!workspace.backend.session().is_dirty());
}

#[test]
fn marquee_selection_uses_containment_left_to_right_and_overlap_right_to_left() {
	let fixture = Fixture::new(2);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_down", [350.0, 250.0], Some("left")),
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_move", [620.0, 350.0], None),
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_up", [620.0, 350.0], Some("left")),
	);
	assert_eq!(selection(&workspace), ["entity-0", "entity-1"]);
	select(&mut workspace, &[]);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_down", [590.0, 290.0], Some("left")),
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_move", [560.0, 310.0], None),
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_up", [560.0, 310.0], Some("left")),
	);
	assert_eq!(selection(&workspace), ["entity-1"]);
}

#[test]
fn one_completed_gizmo_drag_creates_exactly_one_undo_action() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	let original = workspace.backend.session().authored_document().entities[0]
		.transform
		.clone();
	let start = axis_drag_start(&mut controller, &mut workspace);
	for delta in [10.0, 20.0, 30.0] {
		send(
			&mut controller,
			&mut workspace,
			event("pointer_move", [start[0] + delta, start[1]], None),
		);
	}
	assert!(workspace.backend.session().has_transaction());
	assert!(
		!workspace.backend.session().state().can_undo,
		"Preview samples are not independent undo actions"
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_up", [start[0] + 30.0, start[1]], Some("left")),
	);
	assert!(!workspace.backend.session().has_transaction());
	assert!(workspace.backend.session().state().can_undo);
	assert_ne!(
		workspace.backend.session().authored_document().entities[0].transform,
		original
	);
	command(&mut workspace, Command::Undo);
	assert_eq!(
		workspace.backend.session().authored_document().entities[0].transform,
		original
	);
	assert!(!workspace.backend.session().state().can_undo);
}

#[test]
fn native_transform_motion_publishes_only_a_compact_local_preview_until_commit() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	let start = axis_drag_start(&mut controller, &mut workspace);
	assert!(
		controller.take_document_changed(),
		"The begin boundary must refresh the transaction owner"
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_move", [start[0] + 24.0, start[1]], None),
	);
	let state = controller.state();
	let preview = state
		.transform_preview
		.expect("Live Inspector requires a compact native preview");
	let document = workspace.backend.session().authored_document();
	assert_eq!(preview.key, document.entities[0].key.as_str());
	assert_eq!(
		preview.transform.position,
		document.entities[0].transform.position
	);
	assert_eq!(
		preview.transform.rotation,
		document.entities[0].transform.rotation
	);
	assert_eq!(
		preview.transform.scale,
		document.entities[0].transform.scale
	);
	assert_ne!(preview.transform.position, [0.0; 3]);
	assert!(
		!controller.take_document_changed(),
		"Pointer motion must not request a full authored snapshot"
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_up", [start[0] + 24.0, start[1]], Some("left")),
	);
	assert!(controller.state().transform_preview.is_none());
	assert!(
		controller.take_document_changed(),
		"The commit boundary must publish authoritative history and dirty state"
	);
	assert!(!controller.take_document_changed());
}

#[test]
fn escape_and_focus_loss_restore_the_captured_transform_without_history() {
	for boundary in ["escape", "blurred"] {
		let fixture = Fixture::new(1);
		let mut workspace = fixture.open();
		let mut controller = controller(&mut workspace);
		focus_keyboard(&mut controller, &mut workspace);
		let original = workspace.backend.session().authored_document().entities[0]
			.transform
			.clone();
		let start = axis_drag_start(&mut controller, &mut workspace);
		send(
			&mut controller,
			&mut workspace,
			event("pointer_move", [start[0] + 30.0, start[1]], None),
		);
		send(
			&mut controller,
			&mut workspace,
			if boundary == "escape" {
				key("Escape")
			} else {
				event("blurred", start, None)
			},
		);
		assert!(!workspace.backend.session().has_transaction());
		assert_eq!(
			workspace.backend.session().authored_document().entities[0].transform,
			original
		);
		assert!(!workspace.backend.session().state().can_undo);
		assert!(!controller.captured());
		assert!(controller.state().transform_preview.is_none());
	}
}

#[test]
fn unrelated_button_release_does_not_commit_or_lose_the_primary_gesture() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	let start = axis_drag_start(&mut controller, &mut workspace);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_move", [start[0] + 25.0, start[1]], None),
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_up", start, Some("right")),
	);
	assert!(workspace.backend.session().has_transaction());
	assert!(controller.drag.is_some());
	assert!(!workspace.backend.session().state().can_undo);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_up", [start[0] + 25.0, start[1]], Some("left")),
	);
	assert!(!workspace.backend.session().has_transaction());
	assert!(workspace.backend.session().state().can_undo);
}

#[test]
fn secondary_pointer_press_cannot_replace_an_active_transform_or_click() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	let start = axis_drag_start(&mut controller, &mut workspace);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_down", start, Some("middle")),
	);
	assert!(controller.drag.is_some());
	assert!(controller.navigation.is_none());
	controller.cancel(&mut workspace).unwrap();
	select(&mut workspace, &[]);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_down", [10.0, 10.0], Some("left")),
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_down", [10.0, 10.0], Some("right")),
	);
	assert!(controller.click.is_some());
	assert!(controller.navigation.is_none());
	controller.cancel(&mut workspace).unwrap();
}

#[test]
fn changing_tool_with_a_shortcut_cancels_its_active_transform() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	focus_keyboard(&mut controller, &mut workspace);
	let original = workspace.backend.session().authored_document().entities[0]
		.transform
		.clone();
	let start = axis_drag_start(&mut controller, &mut workspace);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_move", [start[0] + 20.0, start[1]], None),
	);
	send(&mut controller, &mut workspace, key("KeyE"));
	assert_eq!(controller.settings.tool, ViewportTool::Rotate);
	assert!(!workspace.backend.session().has_transaction());
	assert_eq!(
		workspace.backend.session().authored_document().entities[0].transform,
		original
	);
	assert!(!workspace.backend.session().state().can_undo);
}

#[test]
fn resizing_or_changing_dpi_cancels_capture_and_does_not_commit() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	let original = workspace.backend.session().authored_document().entities[0]
		.transform
		.clone();
	let start = axis_drag_start(&mut controller, &mut workspace);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_move", [start[0] + 20.0, start[1]], None),
	);
	controller
		.geometry_changed(&mut workspace, [1200, 900], 1.5)
		.unwrap();
	assert!(!workspace.backend.session().has_transaction());
	assert!(!controller.captured());
	assert_eq!(
		workspace.backend.session().authored_document().entities[0].transform,
		original
	);
	assert!(!workspace.backend.session().state().can_undo);
}

#[test]
fn invalid_or_cancelled_model_placement_never_creates_authored_entities() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	let before = serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap();
	assert!(controller
		.action(
			ViewportActionDto::BeginPlacement {
				path: "../outside.glb".into()
			},
			&mut workspace,
			EXTENT
		)
		.is_err());
	controller
		.action(
			ViewportActionDto::BeginPlacement {
				path: "models/triangle.glb".into(),
			},
			&mut workspace,
			EXTENT,
		)
		.unwrap();
	controller
		.action(
			ViewportActionDto::UpdatePlacement { normalized: None },
			&mut workspace,
			EXTENT,
		)
		.unwrap();
	assert!(controller
		.action(ViewportActionDto::CommitPlacement, &mut workspace, EXTENT)
		.is_err());
	controller
		.action(ViewportActionDto::CancelPlacement, &mut workspace, EXTENT)
		.unwrap();
	assert_eq!(
		serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap(),
		before
	);
	assert!(controller.state().placement.is_none());
	controller
		.action(
			ViewportActionDto::Orient {
				orientation: ViewportOrientation::Top,
			},
			&mut workspace,
			EXTENT,
		)
		.unwrap();
	controller
		.action(
			ViewportActionDto::BeginPlacement {
				path: "models/triangle.glb".into(),
			},
			&mut workspace,
			EXTENT,
		)
		.unwrap();
	controller
		.action(
			ViewportActionDto::UpdatePlacement {
				normalized: Some([0.5, 0.5]),
			},
			&mut workspace,
			EXTENT,
		)
		.unwrap();
	assert!(controller.state().placement.unwrap().position.is_some());
	controller
		.action(ViewportActionDto::CancelPlacement, &mut workspace, EXTENT)
		.unwrap();
	assert_eq!(
		serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap(),
		before
	);
}

#[test]
fn resizing_reprojects_an_active_placement_and_preserves_the_submitted_position() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	begin_valid_placement(&mut controller, &mut workspace);
	let normalized = [0.8, 0.5];
	controller
		.action(
			ViewportActionDto::UpdatePlacement {
				normalized: Some(normalized),
			},
			&mut workspace,
			EXTENT,
		)
		.unwrap();
	let original = controller.placement.as_ref().unwrap().position.unwrap();
	let resized = [1200, 600];
	controller
		.geometry_changed(&mut workspace, resized, 1.0)
		.unwrap();
	let placement = controller.placement.as_ref().unwrap();
	assert_eq!(placement.normalized, Some(normalized));
	let position = placement.position.unwrap();
	assert!(position.abs_diff_eq(Vec3::new(original.x * 1.5, original.y, original.z), 1e-4));

	controller
		.action(ViewportActionDto::CommitPlacement, &mut workspace, resized)
		.unwrap();
	let ticket = controller.import.as_ref().unwrap().job_id;
	controller
		.geometry_changed(&mut workspace, [900, 600], 1.0)
		.unwrap();
	assert_eq!(
		controller.placement.as_ref().unwrap().position,
		Some(position)
	);
	drain_until_complete(&mut controller, &mut workspace, ticket);
	assert!(controller.state().error.is_none());
	let document = workspace.backend.session().authored_document();
	assert_eq!(document.entities.len(), 2);
	assert_eq!(document.entities[1].transform.position, position.to_array());
}

#[test]
fn a_valid_drop_adopts_one_import_and_one_undo_restores_the_authored_document() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	let original = workspace.backend.session().authored_document();
	begin_valid_placement(&mut controller, &mut workspace);
	controller
		.action(ViewportActionDto::CommitPlacement, &mut workspace, EXTENT)
		.unwrap();
	let ticket = controller.import.as_ref().unwrap().job_id;
	assert_eq!(controller.state().gesture.as_deref(), Some("asset_loading"));
	drain_until_complete(&mut controller, &mut workspace, ticket);
	assert!(
		controller.state().error.is_none(),
		"{:?}",
		controller.state().error
	);
	assert!(controller.state().placement.is_none());
	assert!(controller.import.is_none());
	assert_eq!(
		workspace
			.backend
			.session()
			.authored_document()
			.entities
			.len(),
		original.entities.len() + 1
	);
	assert_eq!(workspace.backend.session().selection().len(), 1);
	assert_ne!(
		workspace.backend.session().selection()[0],
		original.entities[0].key
	);
	assert!(controller.take_document_changed());
	assert!(!controller.take_document_changed());
	assert!(workspace.backend.session().state().can_undo);
	command(&mut workspace, Command::Undo);
	assert_eq!(*workspace.backend.session().authored_document(), *original);
	assert!(!workspace.backend.session().state().can_undo);
}

#[test]
fn native_placement_reconciles_a_completion_already_adopted_by_the_ui_notifier_once() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	begin_valid_placement(&mut controller, &mut workspace);
	controller
		.action(ViewportActionDto::CommitPlacement, &mut workspace, EXTENT)
		.unwrap();
	let ticket = controller.import.as_ref().unwrap().job_id;
	let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
	loop {
		workspace.drain_asset_jobs();
		if let Some(completion) = workspace
			.recent_completions()
			.into_iter()
			.find(|completion| completion.job_id == ticket)
		{
			assert!(completion.accepted, "{:?}", completion.error);
			break;
		}
		assert!(
			std::time::Instant::now() < deadline,
			"UI notifier did not receive the small fixture import"
		);
		std::thread::sleep(std::time::Duration::from_millis(1));
	}
	assert_eq!(
		workspace
			.backend
			.session()
			.authored_document()
			.entities
			.len(),
		2
	);
	assert!(
		controller.import.is_some(),
		"The native controller has not consumed the UI notification yet"
	);
	assert!(!controller.take_document_changed());
	controller.drain_imports(&mut workspace);
	assert!(controller.import.is_none());
	assert!(controller.placement.is_none());
	assert!(controller.state().gesture.is_none());
	assert!(controller.take_document_changed());
	let revision = controller.state().revision;
	controller.drain_imports(&mut workspace);
	assert!(!controller.take_document_changed());
	assert_eq!(controller.state().revision, revision);
}

#[test]
fn cancelling_a_submitted_drop_before_adoption_cannot_create_a_late_instance() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	let original = workspace.backend.session().authored_document();
	begin_valid_placement(&mut controller, &mut workspace);
	controller
		.action(ViewportActionDto::CommitPlacement, &mut workspace, EXTENT)
		.unwrap();
	let ticket = controller.import.as_ref().unwrap().job_id;
	controller
		.action(ViewportActionDto::CancelPlacement, &mut workspace, EXTENT)
		.unwrap();
	drain_until_complete(&mut controller, &mut workspace, ticket);
	assert!(controller.state().placement.is_none());
	assert!(controller.import.is_none());
	assert_eq!(*workspace.backend.session().authored_document(), *original);
	assert!(!workspace.backend.session().state().can_undo);
	assert!(!controller.take_document_changed());
	let completion = workspace
		.recent_completions()
		.into_iter()
		.find(|completion| completion.job_id == ticket)
		.unwrap();
	assert!(!completion.accepted);
	assert_eq!(completion.error_code.as_deref(), Some("cancelled"));
}

#[test]
fn native_move_under_a_nonuniform_rotated_parent_preserves_the_childs_authored_basis() {
	let fixture = Fixture::new(2);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	command(
		&mut workspace,
		Command::SetTransforms {
			edits: vec![
				TransformEditDto {
					key: "entity-0".into(),
					transform: TransformDto {
						position: [0.0; 3],
						rotation: [0.0, 0.35, 0.0],
						scale: [2.0, 1.0, 0.5],
					},
				},
				TransformEditDto {
					key: "entity-1".into(),
					transform: TransformDto {
						position: [1.5, 0.0, 0.0],
						rotation: [0.0, 0.7, 0.0],
						scale: [1.0; 3],
					},
				},
			],
			space: TransformSpace::Local,
			snap: false,
		},
	);
	command(
		&mut workspace,
		Command::Reparent {
			key: "entity-1".into(),
			parent: Some("entity-0".into()),
			keep_world: false,
		},
	);
	cache_targets(&mut controller, &workspace);
	select(&mut workspace, &["entity-1"]);
	let before = workspace.backend.session().authored_document();
	let world_before = before.world_matrices().unwrap()[1].w_axis.truncate();
	let gizmo = controller.gizmo(&workspace, EXTENT, 1.0).unwrap().unwrap();
	let start = pixel(&controller, gizmo.pivot + gizmo.axes[0] * gizmo.size * 0.7);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_down", start, Some("left")),
	);
	assert!(controller.drag.is_some());
	send(
		&mut controller,
		&mut workspace,
		event("pointer_move", [start[0] + 24.0, start[1]], None),
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_up", [start[0] + 24.0, start[1]], Some("left")),
	);
	let after = workspace.backend.session().authored_document();
	let world_after = after.world_matrices().unwrap()[1].w_axis.truncate();
	assert!(world_after.x > world_before.x);
	assert!((world_after.y - world_before.y).abs() < 1e-4);
	assert!((world_after.z - world_before.z).abs() < 1e-4);
	assert_eq!(after.entities[0].transform, before.entities[0].transform);
	assert_eq!(
		after.entities[1].transform.rotation,
		before.entities[1].transform.rotation
	);
	assert_eq!(
		after.entities[1].transform.scale,
		before.entities[1].transform.scale
	);
	command(&mut workspace, Command::Undo);
	assert_eq!(*workspace.backend.session().authored_document(), *before);
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

#[test]
fn scene_switches_restore_editor_camera_bookmarks_without_authoring_camera_data() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	let first_id = workspace.backend.session().active_scene_id().clone();
	let second_id = runtime::scene_catalog::SceneId::new("second").unwrap();
	let document = (*workspace.backend.session().authored_document()).clone();
	command(
		&mut workspace,
		Command::AddScene {
			id: second_id.clone(),
			asset_path: "scenes/second.json".into(),
			document,
		},
	);
	controller.camera.pivot = Vec3::new(2.0, 3.0, 4.0);
	controller.camera.distance = 7.0;
	let first_pose = controller.camera.position();
	command(
		&mut workspace,
		Command::SwitchScene {
			scene_id: second_id,
		},
	);
	controller.sync(&mut workspace).unwrap();
	controller.camera.pivot = Vec3::new(-5.0, 8.0, 1.0);
	controller.camera.distance = 12.0;
	command(&mut workspace, Command::SwitchScene { scene_id: first_id });
	let authored_camera = workspace
		.backend
		.session()
		.authored_document()
		.camera
		.clone();
	let revision = workspace.backend.session().revision();
	controller.sync(&mut workspace).unwrap();
	assert!((controller.camera.position() - first_pose).length() < 1e-5);
	assert_eq!(
		workspace.backend.session().authored_document().camera,
		authored_camera
	);
	assert_eq!(workspace.backend.session().revision(), revision);
}

#[test]
fn a_4096_entity_selection_has_a_bounded_visual_payload() {
	let fixture = Fixture::new(4096);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	let keys = workspace
		.backend
		.session()
		.authored_document()
		.entities
		.iter()
		.map(|entity| entity.key.as_str().to_owned())
		.collect();
	command(&mut workspace, Command::Select { keys });
	for tool in [
		ViewportTool::Move,
		ViewportTool::Rotate,
		ViewportTool::Scale,
	] {
		controller
			.action(ViewportActionDto::SetTool { tool }, &mut workspace, EXTENT)
			.unwrap();
		let visuals = controller.visuals(&workspace, EXTENT, 2.0).unwrap();
		assert!(visuals.grid.is_some());
		assert!(
			visuals.lines.len() < 512,
			"Selected groups must use bounded group decoration"
		);
		visuals.validate().unwrap();
	}
}

#[test]
fn play_mode_locks_authored_transforms_placement_and_delete() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	focus_keyboard(&mut controller, &mut workspace);
	select(&mut workspace, &["entity-0"]);
	command(&mut workspace, Command::StartPlay);
	controller.sync(&mut workspace).unwrap();
	let authored = workspace.backend.session().authored_document();
	assert!(controller.gizmo(&workspace, EXTENT, 1.0).unwrap().is_none());
	assert!(controller
		.action(
			ViewportActionDto::BeginPlacement {
				path: "models/triangle.glb".into()
			},
			&mut workspace,
			EXTENT
		)
		.is_err());
	send(&mut controller, &mut workspace, key("Delete"));
	assert_eq!(*workspace.backend.session().authored_document(), *authored);
	assert!(!workspace.backend.session().has_transaction());
	assert!(!workspace.backend.session().is_dirty());
}

#[test]
fn scene_keyboard_requires_focus_and_preserves_transform_shortcuts() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	let initial = controller.settings.tool;
	send(&mut controller, &mut workspace, key("KeyR"));
	assert_eq!(controller.settings.tool, initial);
	focus_keyboard(&mut controller, &mut workspace);
	for (code, tool) in [
		("KeyQ", ViewportTool::Hand),
		("KeyW", ViewportTool::Move),
		("KeyE", ViewportTool::Rotate),
		("KeyR", ViewportTool::Scale),
	] {
		send(&mut controller, &mut workspace, key(code));
		assert_eq!(controller.settings.tool, tool);
	}
	send(&mut controller, &mut workspace, key("Escape"));
	send(&mut controller, &mut workspace, key("KeyQ"));
	assert!(!controller.state().focused);
	assert_eq!(controller.settings.tool, ViewportTool::Scale);
}

#[test]
fn middle_mouse_modifiers_choose_orbit_pan_and_dolly() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	let initial = controller.camera.clone();
	let start = [300.0, 300.0];
	let end = [340.0, 330.0];
	send(
		&mut controller,
		&mut workspace,
		event("pointer_down", start, Some("middle")),
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_up", start, Some("right")),
	);
	assert!(controller.captured());
	send(
		&mut controller,
		&mut workspace,
		event("pointer_move", end, None),
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_up", end, Some("middle")),
	);
	assert!(!controller.captured());
	assert_ne!(controller.camera.yaw, initial.yaw);
	assert_eq!(controller.camera.pivot, initial.pivot);
	assert_eq!(controller.camera.distance, initial.distance);
	let orbit = controller.camera.clone();
	let mut pan = event("pointer_down", start, Some("middle"));
	pan.modifiers.shift = true;
	send(&mut controller, &mut workspace, pan);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_move", end, None),
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_up", end, Some("middle")),
	);
	assert_eq!(controller.camera.yaw, orbit.yaw);
	assert_eq!(controller.camera.distance, orbit.distance);
	assert_ne!(controller.camera.pivot, orbit.pivot);
	let panned = controller.camera.clone();
	let mut dolly = event("pointer_down", start, Some("middle"));
	dolly.modifiers.control = true;
	send(&mut controller, &mut workspace, dolly);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_move", end, None),
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_up", end, Some("middle")),
	);
	assert_eq!(controller.camera.pivot, panned.pivot);
	assert!(controller.camera.distance > panned.distance);
	assert!(controller.camera.ortho_height > panned.ortho_height);
}

#[test]
fn walk_uses_wasd_and_mouse_look_and_only_explicit_cancel_restores_the_camera() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	focus_keyboard(&mut controller, &mut workspace);
	send(&mut controller, &mut workspace, key("KeyE"));
	let initial = controller.camera.clone();
	for boundary in ["Escape", "right", "Enter"] {
		start_walk(&mut controller, &mut workspace);
		controller.look_from_fly_motion([50.0, 30.0]);
		assert_ne!(controller.camera.yaw, initial.yaw);
		assert!(controller
			.camera
			.position()
			.abs_diff_eq(initial.position(), 1.0e-5));
		send(&mut controller, &mut workspace, key("KeyW"));
		assert!(controller.needs_camera_tick());
		controller.camera_tick(0.05);
		assert!(!controller
			.camera
			.position()
			.abs_diff_eq(initial.position(), 1.0e-5));
		assert_eq!(controller.settings.tool, ViewportTool::Rotate);
		if boundary == "right" {
			send(
				&mut controller,
				&mut workspace,
				event("pointer_down", [300.0, 300.0], Some("right")),
			);
		} else {
			send(&mut controller, &mut workspace, key(boundary));
		}
		assert!(!controller.captured());
		assert!(!controller.needs_camera_tick());
		assert_eq!(controller.camera.yaw == initial.yaw, boundary != "Enter");
		if boundary == "Escape" {
			assert!(!controller.state().focused);
			focus_keyboard(&mut controller, &mut workspace);
		}
	}
	assert!(!workspace.backend.session().is_dirty());
}

#[test]
fn numpad_changes_orientation_and_projection() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	focus_keyboard(&mut controller, &mut workspace);
	send(&mut controller, &mut workspace, key("Numpad1"));
	assert!(controller.camera.forward().abs_diff_eq(-Vec3::Z, 1.0e-5));
	let mut back = key("Numpad1");
	back.modifiers.control = true;
	send(&mut controller, &mut workspace, back);
	assert!(controller.camera.forward().abs_diff_eq(Vec3::Z, 1.0e-5));
	send(&mut controller, &mut workspace, key("Numpad9"));
	assert!(controller.camera.forward().abs_diff_eq(-Vec3::Z, 1.0e-5));
	send(&mut controller, &mut workspace, key("Numpad5"));
	assert_eq!(
		controller.settings.projection,
		ViewportProjection::Perspective
	);
}

#[test]
fn focused_game_routes_held_mapped_movement_until_key_release_or_blur() {
	use runtime::{
		input::{InputProfile, MovementBindings},
		project::BehaviorSpec,
	};

	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut project = workspace.backend.session().project().clone();
	let player = EntityKey::new("entity-0").unwrap();
	project.add_behavior(BehaviorSpec::ControlledMove {
		entity: player.clone(),
		speed: 2.0,
		space: Default::default(),
	});
	project.input_profile = Some(InputProfile {
		movement: MovementBindings::ARROWS,
		..Default::default()
	});
	command(&mut workspace, Command::SetProject { project });
	let mut controller = controller(&mut workspace);
	focus_keyboard(&mut controller, &mut workspace);
	controller
		.action(
			ViewportActionDto::SetView {
				view: ViewportView::Game,
			},
			&mut workspace,
			EXTENT,
		)
		.unwrap();
	assert!(!controller.game_keyboard_active());
	command(&mut workspace, Command::StartPlay);
	controller.sync(&mut workspace).unwrap();
	assert!(controller.game_keyboard_active());
	assert_eq!(
		workspace.backend.session().play_controlled_targets(),
		Some(&[player][..])
	);
	let position = |workspace: &EditorWorkspace| {
		workspace.backend.session().render_document().entities[0]
			.transform
			.position
	};
	let tick = |workspace: &mut EditorWorkspace| {
		workspace.backend.tick(0.1).unwrap();
		position(workspace)
	};
	send(&mut controller, &mut workspace, key("KeyD"));
	assert_eq!(tick(&mut workspace), [0.0; 3]);
	send(&mut controller, &mut workspace, key("ArrowRight"));
	assert!(Vec3::from_array(tick(&mut workspace)).abs_diff_eq(Vec3::X * 0.2, 1.0e-5));
	send(
		&mut controller,
		&mut workspace,
		event("pointer_up", [300.0, 300.0], Some("left")),
	);
	let before_release = tick(&mut workspace);
	assert!(Vec3::from_array(before_release).abs_diff_eq(Vec3::X * 0.4, 1.0e-5));
	let key_up = ViewportInputDto {
		kind: "key_up".into(),
		..key("ArrowRight")
	};
	send(&mut controller, &mut workspace, key_up);
	assert_eq!(tick(&mut workspace), before_release);
	send(&mut controller, &mut workspace, key("ArrowRight"));
	let before_blur = tick(&mut workspace);
	assert!(Vec3::from_array(before_blur).abs_diff_eq(Vec3::X * 0.6, 1.0e-5));
	controller
		.input(
			&event("blurred", [0.0; 2], None),
			&mut workspace,
			[0; 2],
			1.0,
		)
		.unwrap();
	assert!(controller.keys.is_empty());
	assert!(!controller.state().focused);
	assert_eq!(tick(&mut workspace), before_blur);
	send(&mut controller, &mut workspace, key("ArrowRight"));
	assert_eq!(tick(&mut workspace), before_blur);
	command(&mut workspace, Command::StopPlay);
	controller.sync(&mut workspace).unwrap();
	assert!(!controller.game_keyboard_active());
	assert_eq!(position(&workspace), [0.0; 3]);
}
