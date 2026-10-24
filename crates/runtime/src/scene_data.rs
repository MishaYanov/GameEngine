//! Compatibility path; CPU implementation lives in engine-core.
pub use engine_core::scene_data::*;

#[cfg(test)]
#[path = "tests_scene_data.rs"]
mod tests;
