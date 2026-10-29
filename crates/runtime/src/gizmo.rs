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

#[derive(Clone, Copy)]
pub struct GizmoPose {
	pub local: Transform,
	pub parent: Mat4,
}

impl From<Transform> for GizmoPose {
	fn from(local: Transform) -> Self {
		Self {
			local,
			parent: Mat4::IDENTITY,
		}
	}
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
	pose: GizmoPose,
	parent_inverse: Mat4,
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
		Self::begin_parented(entity, before.into(), camera, extent, pixel, mode, axis)
	}

	pub fn begin_parented(
		entity: EntityId,
		pose: GizmoPose,
		camera: &Camera,
		extent: [u32; 2],
		pixel: [f64; 2],
		mode: GizmoMode,
		axis: usize,
	) -> Result<Self, String> {
		let before = pose.local;
		CameraRay::from_screen(camera, pixel, extent)?;
		if axis >= 3 {
			return Err("Gizmo axis must be X, Y, or Z".into());
		}
		before.validate()?;
		if !pose.parent.is_finite() || pose.parent.determinant() == 0.0 {
			return Err("Gizmo parent transform must be finite and nonsingular".into());
		}
		let parent_inverse = pose.parent.inverse();
		if !parent_inverse.is_finite() {
			return Err("Gizmo parent inverse must be finite".into());
		}
		let origin = pose.origin();
		let size = length(camera, extent, origin.to_array());
		let vector = pose.axis(mode, axis);
		let view_projection = matrix(camera, extent);
		let screen_axis = project_with_matrix(view_projection, extent, origin + vector * size)
			.ok_or("Gizmo is outside the camera")?
			- project_with_matrix(view_projection, extent, origin)
				.ok_or("Gizmo is outside the camera")?;
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
			pose,
			parent_inverse,
		})
	}

	pub fn preview(&self, pixel: [f64; 2]) -> Result<Transform, String> {
		let mut after = self.before;
		let delta = Vec2::new(pixel[0] as f32, pixel[1] as f32) - self.start;
		if !delta.is_finite() {
			return Err("Gizmo cursor must be finite".into());
		}
		if delta == Vec2::ZERO {
			return Ok(self.before);
		}
		let vector = self.pose.axis(self.mode, self.axis);
		match self.mode {
			GizmoMode::Translate => {
				let amount =
					delta.dot(self.screen_axis) / self.screen_axis.length_squared() * self.size;
				after.position = (Vec3::from_array(self.before.position)
					+ self.parent_inverse.transform_vector3(vector * amount))
				.to_array();
			}
			GizmoMode::Scale => {
				let ratio = (1.0 + delta.dot(self.screen_axis) / self.screen_axis.length_squared())
					.max(0.01);
				after.scale[self.axis] = self.before.scale[self.axis] * ratio;
			}
			GizmoMode::Rotate => {
				let current =
					plane_vector(&self.camera, self.extent, pixel, self.pose.origin(), vector)?;
				let start = self.start_vector.ok_or("Missing rotation start")?;
				let angle = vector.dot(start.cross(current)).atan2(start.dot(current));
				if self.pose.parent == Mat4::IDENTITY {
					let initial = Quat::from_euler(
						EulerRot::XYZ,
						self.before.rotation[0],
						self.before.rotation[1],
						self.before.rotation[2],
					);
					let (x, y, z) =
						(Quat::from_axis_angle(vector, angle) * initial).to_euler(EulerRot::XYZ);
					after.rotation = [x, y, z];
					after.validate()?;
					return Ok(after);
				}
				let origin = self.pose.origin();
				let world = Mat4::from_translation(origin)
					* Mat4::from_quat(Quat::from_axis_angle(vector, angle))
					* Mat4::from_translation(-origin)
					* self.pose.parent
					* self.before.matrix();
				let local =
					crate::scene_data::TransformData::from_matrix(self.parent_inverse * world)?;
				after.rotation = local.rotation;
				after.scale = local.scale;
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
