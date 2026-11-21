//! Compatibility path; CPU implementation lives in engine-core.
pub use engine_core::transform::*;

#[cfg(test)]
#[path = "../tests/unit/transform.rs"]
mod tests;
