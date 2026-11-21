//! Compatibility path; CPU implementation lives in engine-core.
pub use engine_core::time::*;

#[cfg(test)]
#[path = "../tests/unit/time.rs"]
mod tests;
