//! Compatibility path; CPU implementation lives in engine-core.
pub use engine_core::camera_controller::*;

#[cfg(test)]
#[path = "../tests/unit/camera_controller.rs"]
mod tests;
