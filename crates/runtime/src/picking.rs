//! GPU Scene adapter for the renderer-independent CPU camera ray.
use crate::scene::{EntityId, Scene};
pub use engine_core::picking::{CpuCameraRay, CpuCameraRay as CameraRay};

pub fn pick(scene: &Scene, ray: CameraRay) -> Option<EntityId> {
	let mut closest = ray.max_distance;
	let mut selected = None;
	for id in scene.entity_ids() {
		let instance = scene.get(id)?;
		let Some(bounds) = instance.model().bounds() else {
			continue;
		};
		let Ok(matrix) = scene.world_matrix(id) else {
			continue;
		};
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
