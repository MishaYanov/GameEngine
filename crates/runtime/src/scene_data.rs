//! Compatibility path; CPU implementation lives in engine-core.
pub use engine_core::scene_data::*;

#[cfg(test)]
#[path = "../tests/unit/scene_data.rs"]
mod tests;
