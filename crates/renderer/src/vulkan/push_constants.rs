use std::{f32::consts::PI, mem::size_of, slice};

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ModelPushConstants {
    pub model: [f32; 16],
}

impl ModelPushConstants {
    pub const fn identity() -> Self {
        Self {
            model: [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
        }
    }

    pub fn from_2d(translation: [f32; 2], rotation_radians: f32, scale: [f32; 2]) -> Self {
        let cosine = rotation_radians.cos();

        let sine = rotation_radians.sin();

        let [translation_x, translation_y] = translation;

        let [scale_x, scale_y] = scale;

        Self {
            model: [
                cosine * scale_x,
                sine * scale_x,
                0.0,
                0.0,
                -sine * scale_y,
                cosine * scale_y,
                0.0,
                0.0,
                0.0,
                0.0,
                1.0,
                0.0,
                translation_x,
                translation_y,
                0.0,
                1.0,
            ],
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        unsafe { slice::from_raw_parts((self as *const Self).cast::<u8>(), size_of::<Self>()) }
    }

    pub const fn size() -> u32 {
        size_of::<Self>() as u32
    }

    pub fn degrees_to_radians(degrees: f32) -> f32 {
        degrees * PI / 180.0
    }
}
