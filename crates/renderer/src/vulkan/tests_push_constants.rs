use super::ModelPushConstants;
use std::mem::{offset_of, size_of};

#[test]
fn push_constant_layout_matches_shader_block() {
    // GLSL std430: mat4 at byte 0, vec4 at byte 64.
    assert_eq!(offset_of!(ModelPushConstants, model), 0);
    assert_eq!(offset_of!(ModelPushConstants, base_color_factor), 64);
    assert_eq!(size_of::<ModelPushConstants>(), 80);
}
