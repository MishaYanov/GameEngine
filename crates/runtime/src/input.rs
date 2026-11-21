//! Compatibility path; CPU implementation lives in engine-core.
pub use engine_core::input::*;

#[cfg(test)]
#[path = "../tests/unit/input.rs"]
mod tests;
