//! Compatibility path; CPU implementation lives in engine-core.
pub use engine_core::camera_controller::*;

#[cfg(test)]
#[path = "tests_camera_controller.rs"]
mod tests;
