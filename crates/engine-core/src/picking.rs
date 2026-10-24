use crate::Camera;
use glam::{Vec3, Vec4};

/// A bounded world-space ray beginning at the camera's near clip plane.
#[derive(Debug, Clone, Copy)]
pub struct CpuCameraRay {
	pub origin: Vec3,
	pub direction: Vec3,
	pub max_distance: f32,
}

pub type CameraRay = CpuCameraRay;

impl CpuCameraRay {
	pub fn from_screen(camera: &Camera, pixel: [f64; 2], extent: [u32; 2]) -> Result<Self, String> {
		if extent.contains(&0)
			|| !pixel.iter().all(|value| value.is_finite())
			|| pixel[0] < 0.0
			|| pixel[1] < 0.0
			|| pixel[0] >= extent[0] as f64
			|| pixel[1] >= extent[1] as f64
		{
			return Err("Picking requires a pixel inside a nonempty viewport".into());
		}
		let inverse = camera
			.view_projection(extent[0] as f32 / extent[1] as f32)?
			.inverse();
		let x = (2.0 * pixel[0] / extent[0] as f64 - 1.0) as f32;
		let y = (2.0 * pixel[1] / extent[1] as f64 - 1.0) as f32;
		let unproject = |depth| -> Result<Vec3, String> {
			let point = inverse * Vec4::new(x, y, depth, 1.0);
			if !point.is_finite() || point.w == 0.0 {
				return Err("Invalid unprojected point".into());
			}
			Ok(point.truncate() / point.w)
		};
		let origin = unproject(0.0)?;
		let delta = unproject(1.0)? - origin;
		let max_distance = delta.length();
		if !max_distance.is_finite() || max_distance <= 0.0 {
			return Err("Camera ray has no finite length".into());
		}
		Ok(Self {
			origin,
			direction: delta / max_distance,
			max_distance,
		})
	}
}
