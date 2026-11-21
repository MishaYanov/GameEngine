//! Compatibility path; CPU implementation lives in engine-core.
pub use engine_core::entity_key::*;

#[cfg(test)]
#[path = "../tests/unit/entity_key.rs"]
mod tests;
