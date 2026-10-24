//! Compatibility path; CPU implementation lives in engine-core.
pub use engine_core::transform::*;

#[cfg(test)]
#[path = "tests_transform.rs"]
mod tests;
