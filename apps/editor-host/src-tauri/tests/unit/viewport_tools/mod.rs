use super::*;

fn approx(a: Vec3, b: Vec3) {
	assert!(a.distance(b) < 0.005, "{a:?} differs from {b:?}");
}
fn pixel(point: Vec2) -> [f64; 2] {
	[point.x as f64, point.y as f64]
}
fn bounds(min: [f32; 3], max: [f32; 3]) -> Bounds3 {
	Bounds3::new(min, max).unwrap()
}
fn camera() -> EditorCamera {
	EditorCamera::default()
}
fn identity_target(key: &str) -> TransformTarget {
	TransformTarget {
		key: key.into(),
		world: Mat4::IDENTITY,
	}
}
fn transform_matrix(edit: &TransformEditDto) -> Mat4 {
	runtime::scene_data::TransformData {
		position: edit.transform.position,
		rotation: edit.transform.rotation,
		scale: edit.transform.scale,
	}
	.matrix()
}
fn axis_drag(
	settings: ViewportSettingsDto,
	tool: ViewportTool,
	targets: Vec<TransformTarget>,
) -> (GizmoDrag, Vec2, Vec2) {
	let camera = camera();
	let extent = [1200, 800];
	let gizmo = Gizmo::new(Vec3::ZERO, Quat::IDENTITY, 2.0);
	let start = camera
		.project(Vec3::ZERO, extent, settings.projection)
		.unwrap();
	let axis = camera
		.project(Vec3::X * 2.0, extent, settings.projection)
		.unwrap()
		- start;
	let drag = GizmoDrag::begin(
		&camera,
		extent,
		settings.projection,
		pixel(start),
		gizmo,
		tool,
		Handle::Axis(0),
		&settings,
		targets,
	)
	.unwrap();
	(drag, start, axis)
}

fn v3_distance_to_line(point: Vec3, origin: Vec3, direction: Vec3) -> f32 {
	(point - origin - direction * (point - origin).dot(direction)).length()
}

mod camera;
mod gizmo;
mod picking;
