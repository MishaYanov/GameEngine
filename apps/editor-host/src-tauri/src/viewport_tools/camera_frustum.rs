use super::*;
use runtime::scene_data::CameraData;

const EDGES: [(usize, usize); 12] = [
	(0, 1),
	(1, 2),
	(2, 3),
	(3, 0),
	(4, 5),
	(5, 6),
	(6, 7),
	(7, 4),
	(0, 4),
	(1, 5),
	(2, 6),
	(3, 7),
];

pub fn camera_frustum(camera: &CameraData, aspect: f32) -> Option<[(Vec3, Vec3); 12]> {
	if !aspect.is_finite() || aspect <= 0.0 {
		return None;
	}
	let native = camera.to_camera();
	let origin = Vec3::from_array(native.position);
	let forward = Vec3::from_array(native.forward());
	let right = Vec3::from_array(native.right());
	let up = right.cross(forward);
	let tangent = (camera.vertical_fov as f64 * 0.5).tan();
	let screen_corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
	let corners: [Vec3; 8] = std::array::from_fn(|index| {
		let depth = if index < 4 {
			camera.near_plane
		} else {
			camera.far_plane
		};
		let height = f64::from(depth) * tangent;
		let (x, y) = screen_corners[index % 4];
		let half_width = height * f64::from(aspect);
		(origin.as_dvec3()
			+ forward.as_dvec3() * f64::from(depth)
			+ right.as_dvec3() * (x * half_width)
			+ up.as_dvec3() * (y * height))
			.as_vec3()
	});
	if corners.iter().any(|corner| !corner.is_finite()) {
		return None;
	}
	Some(EDGES.map(|(a, b)| (corners[a], corners[b])))
}
