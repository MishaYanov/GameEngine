//! Compatibility path; CPU implementation lives in engine-core.
mod gamepad_owner;
#[cfg(windows)]
mod windows_gamepad;

pub use engine_core::input::*;
pub use gamepad_owner::GamepadOwner;
#[cfg(windows)]
pub use windows_gamepad::poll_gamepad;

#[cfg(test)]
#[path = "../tests/unit/input.rs"]
mod tests;
