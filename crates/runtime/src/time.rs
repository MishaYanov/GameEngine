//! Compatibility path; CPU implementation lives in engine-core.
pub use engine_core::time::*;

#[cfg(test)]
#[path = "tests_time.rs"]
mod tests;
