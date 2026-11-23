mod action_map;
mod button;
mod control_graph;
mod gamepad;
mod key;
mod mouse;
mod profile;

pub use action_map::{
	ActionBinding, ConditionalInputLayout, INPUT_ACTION_MAP_SUFFIX, InputActionKind, InputActionLayout,
	InputActionMap, InputBindingOverride, InputLayoutCondition, MAX_CONDITIONAL_INPUT_LAYOUTS,
	MAX_INPUT_ACTION_MAP_BYTES, NamedInputAction, ResolvedInputActions,
};
pub use button::InputButton;
pub use control_graph::{
	CONTROL_GRAPH_SUFFIX, CompiledControlGraph, ControlActionSource, ControlButton, ControlGraph,
	ControlGraphFrame, ControlGraphOutput, ControlIntent, ControlLook, ControlNode,
	ControlNodeData, ControlNodePosition, MAX_CONTROL_GRAPH_BYTES, control_action_types,
};
pub use gamepad::{
	ControllerBindings, FaceButton, GamepadSample, MenuButtons, PhysicalGamepadSample,
};
pub use key::{Key, NativeKeyCode};
pub use mouse::MouseButton;
pub(crate) use profile::HeldGameActions;
pub use profile::{
	GameInputBinding, GameKeyboardInput, INPUT_PROFILE_SCHEMA_VERSION, InputProfile,
	MAX_GAME_INPUT_ACTIONS, MAX_INPUT_PROFILE_BYTES, MovementBindings, valid_game_action_id,
};

use std::collections::HashSet;

#[derive(Clone, Default)]
pub struct InputState {
	held: HashSet<InputButton>,
	pressed: HashSet<InputButton>,
	released: HashSet<InputButton>,
	suppressed: HashSet<InputButton>,
}

impl InputState {
	pub fn press(&mut self, button: impl Into<InputButton>) {
		let key = button.into();
		if self.suppressed.contains(&key) {
			return;
		}
		if self.held.insert(key) {
			self.pressed.insert(key);
		}
	}

	pub fn release(&mut self, button: impl Into<InputButton>) {
		let key = button.into();
		self.suppressed.remove(&key);
		if self.held.remove(&key) {
			self.released.insert(key);
		}
	}

	pub fn is_held(&self, button: impl Into<InputButton>) -> bool {
		self.held.contains(&button.into())
	}

	pub fn just_pressed(&self, button: impl Into<InputButton>) -> bool {
		self.pressed.contains(&button.into())
	}

	pub fn just_released(&self, button: impl Into<InputButton>) -> bool {
		self.released.contains(&button.into())
	}

	pub fn end_frame(&mut self) {
		self.pressed.clear();
		self.released.clear();
	}
	/// Clear motion/edges at a mode boundary without rearming held shortcuts.
	pub fn suppress_held(&mut self) {
		self.suppressed.extend(self.held.drain());
		self.pressed.clear();
		self.released.clear();
	}
	pub fn reset(&mut self) {
		self.held.clear();
		self.pressed.clear();
		self.released.clear();
		self.suppressed.clear();
	}
}
