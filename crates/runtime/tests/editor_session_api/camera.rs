use super::support::{close, entity, key, scene};
use super::{reject, run, select};
use bridge::editor::EditorCommandDto as Command;
use runtime::Bounds3;
use runtime::editor::EditorSession;

#[test]
fn camera_frame_orbit_pan_dolly_use_bounds_and_do_not_rebuild_scene_geometry() {
	let mut s = EditorSession::new(scene(vec![entity("a", [4.0, 2.0, 0.0])])).unwrap();
	select(&mut s, &["a"]);
	reject(&mut s, Command::FrameSelection);
	s.set_model_bounds(
		"props/robot.glb".into(),
		0,
		Bounds3::new([-1.0; 3], [1.0; 3]).unwrap(),
	);
	let original = s.authored_document();
	let geometry_revision = s.scene_revision();
	run(&mut s, Command::FrameSelection);
	let framed = s.authored_document();
	close(framed.camera.position[0], 4.0);
	close(framed.camera.position[1], 2.0);
	assert!(framed.camera.position[2] > 1.0);
	run(
		&mut s,
		Command::OrbitCamera {
			yaw: 0.5,
			pitch: 0.2,
		},
	);
	let camera = &s.authored_document().camera;
	let distance = glam::Vec3::from_array(camera.position).distance(glam::Vec3::new(4.0, 2.0, 0.0));
	let previous =
		glam::Vec3::from_array(framed.camera.position).distance(glam::Vec3::new(4.0, 2.0, 0.0));
	close(distance, previous);
	run(
		&mut s,
		Command::PanCamera {
			right: 1.0,
			up: 0.5,
		},
	);
	run(&mut s, Command::DollyCamera { distance: 0.2 });
	assert_eq!(s.scene_revision(), geometry_revision);
	reject(
		&mut s,
		Command::OrbitCamera {
			yaw: f32::NAN,
			pitch: 0.0,
		},
	);
	for _ in 0..4 {
		run(&mut s, Command::Undo);
	}
	assert_eq!(s.authored_document(), original);
}

#[test]
fn picking_skips_transform_only_groups_and_framing_uses_descendant_geometry() {
	let group = entity("group", [0.0; 3]).with_renderable(false);
	let child = entity("child", [0.0; 3])
		.with_parent(Some(key("group")))
		.with_mesh_index(Some(1));
	let mut s = EditorSession::new(scene(vec![group, child])).unwrap();
	s.set_model_bounds(
		"props/robot.glb".into(),
		0,
		Bounds3::new([-100.0; 3], [100.0; 3]).unwrap(),
	);
	s.set_model_bounds(
		"props/robot.glb".into(),
		1,
		Bounds3::new([-0.5; 3], [0.5; 3]).unwrap(),
	);
	run(
		&mut s,
		Command::PickSelection {
			pixel: [400.0, 300.0],
			extent: [800, 600],
			additive: false,
		},
	);
	assert_eq!(s.selection(), &[key("child")]);
	select(&mut s, &["group"]);
	run(&mut s, Command::FrameSelection);
	let framed_group = s.authored_document().camera.clone();
	run(&mut s, Command::Undo);
	select(&mut s, &["child"]);
	run(&mut s, Command::FrameSelection);
	assert_eq!(s.authored_document().camera, framed_group);
	assert!(
		framed_group.position[2] < 10.0,
		"framed phantom mesh-0 geometry"
	);
}
