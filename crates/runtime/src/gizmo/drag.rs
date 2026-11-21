//! Snapshots a gizmo drag and resolves absolute previews from its starting pose.

use super::{GizmoMode, GizmoPose, length, matrix, project_with_matrix};
use crate::{Camera, Transform, picking::CameraRay, scene::EntityId};
use glam::{EulerRot, Mat4, Quat, Vec2, Vec3};

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
