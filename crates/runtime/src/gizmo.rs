use crate::{Camera, Transform, picking::CameraRay, scene::EntityId};
use glam::{EulerRot, Mat4, Quat, Vec2, Vec3};
use renderer::vulkan::CameraUniform;

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
	let clip = matrix(camera, extent) * point.extend(1.0);
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
	let origin = Vec3::from_array(transform.position);
	if project(camera, extent, origin).is_none() {
		return Vec::new();
	}
	let size = length(camera, extent, transform.position);
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
			let vector = axis_vector(transform, mode, axis);
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
	let pixel = Vec2::new(pixel[0] as f32, pixel[1] as f32);
	let mut nearest = 9.0_f32;
	let mut hit = None;
	for segment in segments(camera, extent, transform, mode) {
		let (Some(a), Some(b)) = (
			project(camera, extent, segment.a),
			project(camera, extent, segment.b),
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

pub struct GizmoDrag {
	pub entity: EntityId,
	pub before: Transform,
	pub axis: usize,
	mode: GizmoMode,
	camera: Camera,
	extent: [u32; 2],
	start: Vec2,
	screen_axis: Vec2,
	size: f32,
	start_vector: Option<Vec3>,
}

impl GizmoDrag {
	pub fn begin(
		entity: EntityId,
		before: Transform,
		camera: &Camera,
		extent: [u32; 2],
		pixel: [f64; 2],
		mode: GizmoMode,
		axis: usize,
	) -> Result<Self, String> {
		CameraRay::from_screen(camera, pixel, extent)?;
		if axis >= 3 {
			return Err("Gizmo axis must be X, Y, or Z".into());
		}
		before.validate()?;
		let size = length(camera, extent, before.position);
		let vector = axis_vector(before, mode, axis);
		let origin = Vec3::from_array(before.position);
		let screen_axis = project(camera, extent, origin + vector * size)
			.ok_or("Gizmo is outside the camera")?
			- project(camera, extent, origin).ok_or("Gizmo is outside the camera")?;
		if mode != GizmoMode::Rotate && screen_axis.length_squared() < 1.0 {
			return Err("This axis points into the camera; move the camera to drag it".into());
		}
		let start_vector = if mode == GizmoMode::Rotate {
			Some(plane_vector(camera, extent, pixel, origin, vector)?)
		} else {
			None
		};
		Ok(Self {
			entity,
			before,
			axis,
			mode,
			camera: camera.clone(),
			extent,
			start: Vec2::new(pixel[0] as f32, pixel[1] as f32),
			screen_axis,
			size,
			start_vector,
		})
	}

	pub fn preview(&self, pixel: [f64; 2]) -> Result<Transform, String> {
		let mut after = self.before;
		let delta = Vec2::new(pixel[0] as f32, pixel[1] as f32) - self.start;
		if !delta.is_finite() {
			return Err("Gizmo cursor must be finite".into());
		}
		let vector = axis_vector(self.before, self.mode, self.axis);
		match self.mode {
			GizmoMode::Translate => {
				let amount =
					delta.dot(self.screen_axis) / self.screen_axis.length_squared() * self.size;
				after.position =
					(Vec3::from_array(self.before.position) + vector * amount).to_array();
			}
			GizmoMode::Scale => {
				let ratio = (1.0 + delta.dot(self.screen_axis) / self.screen_axis.length_squared())
					.max(0.01);
				after.scale[self.axis] = self.before.scale[self.axis] * ratio;
			}
			GizmoMode::Rotate => {
				let current = plane_vector(
					&self.camera,
					self.extent,
					pixel,
					Vec3::from_array(self.before.position),
					vector,
				)?;
				let start = self.start_vector.ok_or("Missing rotation start")?;
				let angle = vector.dot(start.cross(current)).atan2(start.dot(current));
				let initial = Quat::from_euler(
					EulerRot::XYZ,
					self.before.rotation[0],
					self.before.rotation[1],
					self.before.rotation[2],
				);
				let (x, y, z) =
					(Quat::from_axis_angle(vector, angle) * initial).to_euler(EulerRot::XYZ);
				after.rotation = [x, y, z];
			}
		}
		after.validate()?;
		Ok(after)
	}
}

fn plane_vector(
	camera: &Camera,
	extent: [u32; 2],
	pixel: [f64; 2],
	origin: Vec3,
	normal: Vec3,
) -> Result<Vec3, String> {
	let ray = CameraRay::from_screen(camera, pixel, extent)?;
	let denominator = ray.direction.dot(normal);
	if denominator.abs() < 1e-5 {
		return Err("Rotation plane is edge-on; move the camera".into());
	}
	let t = (origin - ray.origin).dot(normal) / denominator;
	if t < 0.0 {
		return Err("Rotation plane is behind the camera".into());
	}
	(ray.origin + ray.direction * t - origin)
		.try_normalize()
		.ok_or("Drag away from the rotation center".into())
}
