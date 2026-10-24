//! Compatibility path; CPU implementation lives in engine-core.
pub use engine_core::camera::*;

#[cfg(test)]
#[path = "tests_camera.rs"]
mod tests;
