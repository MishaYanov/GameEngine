use crate::{
	Camera,
	scene::{EntityId, Scene},
};
use glam::{Mat4, Vec3, Vec4};
use renderer::vulkan::CameraUniform;

#[derive(Debug, Clone, Copy)]
pub struct CameraRay {
	pub origin: Vec3,
	pub direction: Vec3,
	pub max_distance: f32,
}

impl CameraRay {
	pub fn from_screen(camera: &Camera, pixel: [f64; 2], extent: [u32; 2]) -> Result<Self, String> {
		if extent.contains(&0)
			|| !pixel.iter().all(|value| value.is_finite())
			|| pixel[0] < 0.0
			|| pixel[1] < 0.0
			|| pixel[0] >= extent[0] as f64
			|| pixel[1] >= extent[1] as f64
		{
			return Err("Picking requires a pixel inside a nonempty viewport".to_string());
		}
		if !camera
			.position
			.iter()
			.chain(
				[
					camera.yaw,
					camera.pitch,
					camera.vertical_fov,
					camera.near,
					camera.far,
				]
					.iter(),
			)
			.all(|value| value.is_finite())
			|| camera.near <= 0.0
			|| camera.far <= camera.near
			|| camera.vertical_fov <= 0.0
			|| camera.vertical_fov >= std::f32::consts::PI
		{
			return Err("Picking requires a valid camera".to_string());
		}
		let uniform = CameraUniform::perspective(
			camera.position,
			camera.target(),
			[0.0, 1.0, 0.0],
			camera.vertical_fov,
			extent[0] as f32 / extent[1] as f32,
			camera.near,
			camera.far,
		);
		let matrix = Mat4::from_cols_array(&uniform.view_projection);
		if !matrix.is_finite() || matrix.determinant() == 0.0 {
			return Err("Camera matrix cannot be inverted".to_string());
		}
		let inverse = matrix.inverse();
		// Positive-height Vulkan viewport: screen Y and NDC Y both increase down.
		let x = (2.0 * pixel[0] / extent[0] as f64 - 1.0) as f32;
		let y = (2.0 * pixel[1] / extent[1] as f64 - 1.0) as f32;
		let unproject = |depth| -> Result<Vec3, String> {
			let point = inverse * Vec4::new(x, y, depth, 1.0);
			if !point.is_finite() || point.w == 0.0 {
				return Err("Invalid unprojected point".to_string());
			}
			Ok(point.truncate() / point.w)
		};
		let origin = unproject(0.0)?;
		let delta = unproject(1.0)? - origin;
		let max_distance = delta.length();
		if !max_distance.is_finite() || max_distance <= 0.0 {
			return Err("Camera ray has no finite length".to_string());
		}
		Ok(Self {
			origin,
			direction: delta / max_distance,
			max_distance,
		})
	}
}

pub fn pick(scene: &Scene, ray: CameraRay) -> Option<EntityId> {
	let mut closest = ray.max_distance;
	let mut selected = None;
	for id in scene.entity_ids() {
		let instance = scene.get(id)?;
		let Some(bounds) = instance.model().bounds() else {
			continue;
		};
		let matrix = instance.transform().matrix();
		if !matrix.is_finite() || matrix.determinant() == 0.0 {
			continue;
		}
		let inverse = matrix.inverse();
		let hit = bounds.ray_distance(
			inverse.transform_point3(ray.origin),
			inverse.transform_vector3(ray.direction),
			closest,
		);
		if let Some(distance) = hit {
			// Stable scene order wins exact ties.
			if selected.is_none() || distance < closest {
				closest = distance;
				selected = Some(id);
			}
		}
	}
	selected
}
