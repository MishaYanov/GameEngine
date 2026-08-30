use std::{mem::size_of, slice};

use glam::{EulerRot, Mat4, Quat, Vec3};

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ModelPushConstants {
    pub model: [f32; 16],
}

impl ModelPushConstants {
    pub const OFFSET: u32 = 0;

    pub const SIZE: u32 = size_of::<Self>() as u32;

    pub const fn identity() -> Self {
        Self {
            model: [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
        }
    }

    pub fn from_3d(translation: [f32; 3], rotation_radians: [f32; 3], scale: [f32; 3]) -> Self {
        let translation = Vec3::from_array(translation);

        let scale = Vec3::from_array(scale);

        let rotation = Quat::from_euler(
            EulerRot::XYZ,
            rotation_radians[0],
            rotation_radians[1],
            rotation_radians[2],
        );

        let model = Mat4::from_scale_rotation_translation(scale, rotation, translation);

        Self {
            model: model.to_cols_array(),
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        unsafe { slice::from_raw_parts((self as *const Self).cast::<u8>(), size_of::<Self>()) }
    }

    pub fn degrees_to_radians(degrees: f32) -> f32 {
        degrees.to_radians()
    }
}
