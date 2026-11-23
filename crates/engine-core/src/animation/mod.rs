//! Bounded CPU animation data and deterministic pose evaluation. No asset I/O or GPU ownership.
mod animator_graph;
mod animator_runtime;
mod asset;
mod authored_clip;
mod instance;
mod sample;
mod settings;
pub use asset::*;
pub use authored_clip::*;
pub use instance::*;
pub use sample::*;
pub use settings::*;

pub use animator_graph::*;

pub(crate) use animator_runtime::{AnimatorBank, AnimatorTick};
pub use animator_runtime::{
	AnimatorInspection, AnimatorResolver, AnimatorStepInputs, PreparedAnimator,
	PreparedAnimatorBindings, PreparedAnimatorClips,
};
