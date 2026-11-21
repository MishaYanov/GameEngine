//! Compatibility path; CPU implementation lives in engine-core.
pub use engine_core::camera::*;

#[cfg(test)]
#[path = "../tests/unit/camera.rs"]
mod tests;
