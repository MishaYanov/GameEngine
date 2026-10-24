use super::super::MeshData;
use renderer::vulkan::Vertex;

pub(super) fn build(half_cylinder: f32) -> MeshData {
	const SEGMENTS: u32 = 32;
	const HEMISPHERE_RINGS: u32 = 8;
	let mut profile = Vec::new();
	for ring in 0..=HEMISPHERE_RINGS {
		let angle = std::f32::consts::FRAC_PI_2 * ring as f32 / HEMISPHERE_RINGS as f32;
		profile.push((0.5 * angle.sin(), half_cylinder + 0.5 * angle.cos()));
	}
	let first_bottom = if half_cylinder > 0.0 { 0 } else { 1 };
	for ring in first_bottom..=HEMISPHERE_RINGS {
		let angle = std::f32::consts::FRAC_PI_2 * ring as f32 / HEMISPHERE_RINGS as f32;
		profile.push((0.5 * angle.cos(), -half_cylinder - 0.5 * angle.sin()));
	}
	let last = profile.len() as u32 - 1;
	// Exact pole positions avoid tiny rings caused by floating-point trig.
	profile[0].0 = 0.0;
	profile[last as usize].0 = 0.0;
	let mut vertices = Vec::new();
	let mut indices = Vec::new();
	for (ring, &(radius, y)) in profile.iter().enumerate() {
		for segment in 0..=SEGMENTS {
			let u = segment as f32 / SEGMENTS as f32;
			let angle = if segment == SEGMENTS {
				0.0
			} else {
				std::f32::consts::TAU * u
			};
			vertices.push(Vertex::new(
				[radius * angle.cos(), y, radius * angle.sin()],
				[u, 0.5 - y / (2.0 * half_cylinder + 1.0)],
			));
		}
		if ring as u32 == last {
			continue;
		}
		for segment in 0..SEGMENTS {
			let a = ring as u32 * (SEGMENTS + 1) + segment;
			let b = a + SEGMENTS + 1;
			if ring > 0 {
				indices.extend_from_slice(&[a, a + 1, b]);
			}
			if (ring as u32) < last - 1 {
				indices.extend_from_slice(&[a + 1, b + 1, b]);
			}
		}
	}
	MeshData::new(vertices, indices)
}
