use super::*;
#[test]
fn palette_offsets_preserve_material_flags_and_packed_vertex_contract() {
	use std::mem::{offset_of, size_of};
	assert_eq!(size_of::<super::super::mesh::SkinInfluence>(), 24);
	assert_eq!(offset_of!(super::super::mesh::SkinInfluence, weights), 8);
	let mut push = super::super::ModelPushConstants::identity();
	push.flags[0] = 1;
	push.base_color_factor = [0.2, 0.4, 0.6, 1.];
	apply_palette_range(&mut push, 0, 2);
	apply_palette_range(&mut push, 2, 2);
	assert_eq!(push.flags, [1, 2, 2, 0]);
	assert_eq!(push.base_color_factor, [0.2, 0.4, 0.6, 1.]);
	assert_eq!(offset_of!(super::super::ModelPushConstants, flags), 80);
}
#[test]
fn opposite_rotation_normal_blend_uses_finite_dominant_influence() {
	let transforms = [
		glam::Mat3::IDENTITY,
		glam::Mat3::from_rotation_z(std::f32::consts::PI),
	];
	let normals = transforms.map(|m| (m * glam::Vec3::X).normalize());
	let weighted = normals[0] * 0.5 + normals[1] * 0.5;
	assert!(weighted.length_squared() < 1e-12);
	let result = if weighted.length_squared() > 1e-12 {
		weighted.normalize()
	} else {
		normals[0]
	};
	assert!(result.is_finite());
	assert_eq!(result, glam::Vec3::X);
}
