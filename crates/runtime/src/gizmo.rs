use crate::{Camera, Transform, scene::EntityId};
use glam::{EulerRot, Mat4, Quat, Vec2, Vec3};
use renderer::vulkan::CameraUniform;

mod drag;
pub use drag::GizmoDrag;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GizmoMode {
	#[default]
	Translate,
	Rotate,
	Scale,
}

#[derive(Clone, Copy)]
pub struct GizmoSegment {
	pub a: Vec3,
	pub b: Vec3,
	pub axis: usize,
}

#[derive(Clone, Copy)]
pub struct GizmoPose {
	pub local: Transform,
	pub parent: Mat4,
}

impl GizmoPose {
	pub fn from_scene(scene: &crate::scene::Scene, entity: EntityId) -> Result<Self, String> {
		let local = *scene
			.get(entity)
			.ok_or("Gizmo entity is missing")?
			.transform();
		let parent = match scene.parent(entity) {
			Some(key) => {
				scene.world_matrix(scene.entity_by_key(key).ok_or("Gizmo parent is missing")?)?
			}
			None => Mat4::IDENTITY,
		};
		Ok(Self { local, parent })
	}
	fn origin(self) -> Vec3 {
		self.parent
			.transform_point3(Vec3::from_array(self.local.position))
	}
	fn axis(self, mode: GizmoMode, axis: usize) -> Vec3 {
		let vector = axis_vector(self.local, mode, axis);
		if mode == GizmoMode::Scale {
			self.parent.transform_vector3(vector).normalize_or_zero()
		} else {
			vector
		}
	}
}

impl From<Transform> for GizmoPose {
	fn from(local: Transform) -> Self {
		Self {
			local,
			parent: Mat4::IDENTITY,
		}
	}
}

fn matrix(camera: &Camera, extent: [u32; 2]) -> Mat4 {
	Mat4::from_cols_array(
		&CameraUniform::perspective(
			camera.position,
			camera.target(),
			[0.0, 1.0, 0.0],
			camera.vertical_fov,
			extent[0] as f32 / extent[1] as f32,
			camera.near,
			camera.far,
		)
		.view_projection,
	)
}

pub fn project(camera: &Camera, extent: [u32; 2], point: Vec3) -> Option<Vec2> {
	if extent.contains(&0) {
		return None;
	}
	project_with_matrix(matrix(camera, extent), extent, point)
}

fn project_with_matrix(view_projection: Mat4, extent: [u32; 2], point: Vec3) -> Option<Vec2> {
	let clip = view_projection * point.extend(1.0);
	if !clip.is_finite() || clip.w <= 0.0 || clip.z < 0.0 || clip.z > clip.w {
		return None;
	}
	let ndc = clip.truncate() / clip.w;
	Some(Vec2::new(
		(ndc.x + 1.0) * 0.5 * extent[0] as f32,
		(ndc.y + 1.0) * 0.5 * extent[1] as f32,
	))
}

pub fn length(camera: &Camera, extent: [u32; 2], position: [f32; 3]) -> f32 {
	let distance = (Vec3::from_array(position) - Vec3::from_array(camera.position))
		.dot(Vec3::from_array(camera.forward()));
	(distance * (camera.vertical_fov * 0.5).tan() * 180.0 / extent[1].max(1) as f32).max(0.05)
}

fn axis_vector(transform: Transform, mode: GizmoMode, axis: usize) -> Vec3 {
	let vector = [Vec3::X, Vec3::Y, Vec3::Z][axis];
	if mode == GizmoMode::Scale {
		Quat::from_euler(
			EulerRot::XYZ,
			transform.rotation[0],
			transform.rotation[1],
			transform.rotation[2],
		) * vector
	} else {
		vector
	}
}

pub fn segments(
	camera: &Camera,
	extent: [u32; 2],
	transform: Transform,
	mode: GizmoMode,
) -> Vec<GizmoSegment> {
	segments_parented(camera, extent, transform.into(), mode)
}

pub fn segments_parented(
	camera: &Camera,
	extent: [u32; 2],
	pose: GizmoPose,
	mode: GizmoMode,
) -> Vec<GizmoSegment> {
	if extent.contains(&0) {
		return Vec::new();
	}
	segments_with_matrix(camera, extent, pose, mode, matrix(camera, extent))
}

fn segments_with_matrix(
	camera: &Camera,
	extent: [u32; 2],
	pose: GizmoPose,
	mode: GizmoMode,
	view_projection: Mat4,
) -> Vec<GizmoSegment> {
	let origin = pose.origin();
	if project_with_matrix(view_projection, extent, origin).is_none() {
		return Vec::new();
	}
	let size = length(camera, extent, origin.to_array());
	let mut segments = Vec::new();
	for axis in 0..3 {
		if mode == GizmoMode::Rotate {
			let a = [Vec3::X, Vec3::Y, Vec3::Z][(axis + 1) % 3];
			let b = [Vec3::X, Vec3::Y, Vec3::Z][(axis + 2) % 3];
			for step in 0..48 {
				let angle = std::f32::consts::TAU * step as f32 / 48.0;
				let next = std::f32::consts::TAU * (step + 1) as f32 / 48.0;
				segments.push(GizmoSegment {
					axis,
					a: origin + size * (a * angle.cos() + b * angle.sin()),
					b: origin + size * (a * next.cos() + b * next.sin()),
				});
			}
		} else {
			let vector = pose.axis(mode, axis);
			segments.push(GizmoSegment {
				axis,
				a: origin + vector * size * 0.15,
				b: origin + vector * size,
			});
		}
	}
	segments
}

pub fn hit_test(
	camera: &Camera,
	extent: [u32; 2],
	transform: Transform,
	mode: GizmoMode,
	pixel: [f64; 2],
) -> Option<usize> {
	hit_test_parented(camera, extent, transform.into(), mode, pixel)
}

pub fn hit_test_parented(
	camera: &Camera,
	extent: [u32; 2],
	pose: GizmoPose,
	mode: GizmoMode,
	pixel: [f64; 2],
) -> Option<usize> {
	if extent.contains(&0) {
		return None;
	}
	let view_projection = matrix(camera, extent);
	let pixel = Vec2::new(pixel[0] as f32, pixel[1] as f32);
	let mut nearest = 9.0_f32;
	let mut hit = None;
	for segment in segments_with_matrix(camera, extent, pose, mode, view_projection) {
		let (Some(a), Some(b)) = (
			project_with_matrix(view_projection, extent, segment.a),
			project_with_matrix(view_projection, extent, segment.b),
		) else {
			continue;
		};
		let delta = b - a;
		if delta.length_squared() < 1.0 {
			continue;
		}
		let closest = a + delta * ((pixel - a).dot(delta) / delta.length_squared()).clamp(0.0, 1.0);
		let distance = pixel.distance(closest);
		if distance < nearest {
			nearest = distance;
			hit = Some(segment.axis);
		}
	}
	hit
}
