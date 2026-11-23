use super::{
	FaceButton, GamepadSample, InputProfile, InputState, Key, MovementBindings,
	PhysicalGamepadSample, valid_game_action_id,
};
use crate::asset_catalog::AssetId;
use crate::scene_catalog::{SceneCatalog, SceneId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

pub const MAX_INPUT_ACTION_MAP_BYTES: usize = 64 * 1024;
pub const INPUT_ACTION_MAP_SUFFIX: &str = ".input-actions.json";
const MAX_ACTIONS: usize = super::MAX_GAME_INPUT_ACTIONS;
const MAX_BINDINGS: usize = 8;
pub const MAX_CONDITIONAL_INPUT_LAYOUTS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputActionKind {
	Button,
	Vector2,
	Delta2,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActionBinding {
	Key { key: Key },
	GamepadButton { button: FaceButton },
	KeyboardVector { keys: MovementBindings },
	KeyboardLook { keys: MovementBindings },
	GamepadLeftStick,
	GamepadRightStick,
	PointerDelta,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedInputAction {
	pub id: String,
	pub label: String,
	pub value: InputActionKind,
	pub bindings: Vec<ActionBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InputLayoutCondition {
	CharacterGrounded { value: bool },
	SceneIs { scene_id: SceneId },
}

impl InputLayoutCondition {
	pub fn matches(&self, scene: &SceneId, grounded: Option<bool>) -> bool {
		match self {
			Self::CharacterGrounded { value } => grounded == Some(*value),
			Self::SceneIs { scene_id } => scene == scene_id,
		}
	}
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputBindingOverride {
	pub action_id: String,
	pub bindings: Vec<ActionBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionalInputLayout {
	pub id: String,
	pub label: String,
	pub when: InputLayoutCondition,
	pub overrides: Vec<InputBindingOverride>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputActionMap {
	pub schema_version: u32,
	pub actions: Vec<NamedInputAction>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub layouts: Vec<ConditionalInputLayout>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputActionLayout {
	pub schema_version: u32,
	pub asset_id: AssetId,
	pub binding_digest: String,
	pub overrides: BTreeMap<String, Vec<ActionBinding>>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ResolvedInputActions {
	pub buttons: BTreeSet<String>,
	pub vectors: BTreeMap<String, [f32; 2]>,
	pub deltas: BTreeMap<String, [f32; 2]>,
	pub look_rates: BTreeMap<String, [f32; 2]>,
	pub animation_parameters: BTreeSet<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum PhysicalControl {
	Key(Key),
	Face(u8),
	LeftStick,
	RightStick,
	Pointer,
}

impl ActionBinding {
	fn kind(&self) -> InputActionKind {
		match self {
			Self::Key { .. } | Self::GamepadButton { .. } => InputActionKind::Button,
			Self::KeyboardVector { .. } | Self::GamepadLeftStick => InputActionKind::Vector2,
			Self::PointerDelta | Self::KeyboardLook { .. } | Self::GamepadRightStick => {
				InputActionKind::Delta2
			}
		}
	}

	fn controls(&self) -> [Option<PhysicalControl>; 4] {
		use PhysicalControl as Control;
		let single = |control| [Some(control), None, None, None];
		match *self {
			Self::Key { key } => single(Control::Key(key)),
			Self::GamepadButton { button } => single(Control::Face(button as u8)),
			Self::GamepadLeftStick => single(Control::LeftStick),
			Self::GamepadRightStick => single(Control::RightStick),
			Self::PointerDelta => single(Control::Pointer),
			Self::KeyboardVector { keys } | Self::KeyboardLook { keys } => [
				keys.negative_x,
				keys.positive_x,
				keys.negative_y,
				keys.positive_y,
			]
			.map(|key| Some(Control::Key(key))),
		}
	}
}

fn supported_key(key: Key) -> bool {
	let code = key.physical_code();
	let mut bytes = code.bytes();
	key != Key::Escape
		&& code.len() <= 32
		&& bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
		&& bytes.all(|byte| byte.is_ascii_alphanumeric())
}

impl InputActionMap {
	pub fn starter() -> Self {
		use ActionBinding as Binding;
		let action = |id: &str, label: &str, value, bindings| NamedInputAction {
			id: id.into(),
			label: label.into(),
			value,
			bindings,
		};
		Self {
			schema_version: 1,
			layouts: Vec::new(),
			actions: vec![
				action(
					"move",
					"Move",
					InputActionKind::Vector2,
					vec![
						Binding::KeyboardVector {
							keys: MovementBindings::WASD,
						},
						Binding::GamepadLeftStick,
					],
				),
				action(
					"look",
					"Look",
					InputActionKind::Delta2,
					vec![Binding::PointerDelta, Binding::GamepadRightStick],
				),
				action(
					"jump",
					"Jump",
					InputActionKind::Button,
					vec![
						Binding::Key { key: Key::Space },
						Binding::GamepadButton {
							button: FaceButton::A,
						},
					],
				),
				action(
					"attack",
					"Attack",
					InputActionKind::Button,
					vec![
						Binding::Key { key: Key::F },
						Binding::GamepadButton {
							button: FaceButton::X,
						},
					],
				),
			],
		}
	}

	pub fn from_json(bytes: &[u8]) -> Result<Self, String> {
		if bytes.len() > MAX_INPUT_ACTION_MAP_BYTES {
			return Err("Input Actions asset exceeds 64 KiB".into());
		}
		let map: Self =
			serde_json::from_slice(bytes).map_err(|error| format!("Input Actions: {error}"))?;
		map.validate()?;
		Ok(map)
	}

	pub fn to_json(&self) -> Result<Vec<u8>, String> {
		self.validate()?;
		let bytes = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
		if bytes.len() > MAX_INPUT_ACTION_MAP_BYTES {
			return Err("Formatted Input Actions asset exceeds 64 KiB".into());
		}
		Ok(bytes)
	}

	pub fn validate(&self) -> Result<(), String> {
		if !matches!(self.schema_version, 1 | 2) {
			return Err("Unsupported Input Actions schema version".into());
		}
		if !(1..=MAX_ACTIONS).contains(&self.actions.len()) {
			return Err(format!(
				"Input Actions require between 1 and {MAX_ACTIONS} actions"
			));
		}
		if self.schema_version == 1 && !self.layouts.is_empty() {
			return Err("Conditional Input Actions layouts require schema 2".into());
		}
		if self.layouts.len() > MAX_CONDITIONAL_INPUT_LAYOUTS {
			return Err("Input Actions support up to 16 conditional layouts".into());
		}
		validate_actions(&self.actions)?;
		let mut layout_ids = HashSet::new();
		for layout in &self.layouts {
			if !valid_game_action_id(&layout.id)
				|| layout.id == "default"
				|| !layout_ids.insert(layout.id.as_str())
			{
				return Err(format!(
					"Invalid or duplicate Input Actions layout ID {:?}",
					layout.id
				));
			}
			if !valid_display_label(&layout.label) {
				return Err(format!(
					"Layout {:?} requires a display name of 1–96 bytes",
					layout.id
				));
			}
			if layout.overrides.len() > MAX_ACTIONS {
				return Err(format!(
					"Layout {:?} has too many action overrides",
					layout.id
				));
			}
			let mut override_ids = HashSet::new();
			for entry in &layout.overrides {
				if !valid_game_action_id(&entry.action_id)
					|| !override_ids.insert(entry.action_id.as_str())
					|| entry.bindings.len() > MAX_BINDINGS
				{
					return Err(format!(
						"Layout {:?} has invalid or duplicate action overrides",
						layout.id
					));
				}
			}
			let effective = self.effective_actions(layout)?;
			validate_actions(&effective)
				.map_err(|error| format!("Layout {:?}: {error}", layout.label))?;
		}
		Ok(())
	}

	pub fn validate_scenes(&self, scenes: &SceneCatalog) -> Result<(), String> {
		for layout in &self.layouts {
			if let InputLayoutCondition::SceneIs { scene_id } = &layout.when {
				scenes.get(scene_id).map_err(|_| {
					format!(
						"Layout {:?} references a missing scene {:?}",
						layout.label,
						scene_id.as_str()
					)
				})?;
			}
		}
		Ok(())
	}

	pub fn effective_actions(
		&self,
		layout: &ConditionalInputLayout,
	) -> Result<Vec<NamedInputAction>, String> {
		let mut overrides: HashMap<_, _> = layout
			.overrides
			.iter()
			.map(|entry| (entry.action_id.as_str(), &entry.bindings))
			.collect();
		let mut actions = self.actions.clone();
		for action in &mut actions {
			if let Some(bindings) = overrides.remove(action.id.as_str()) {
				action.bindings.clone_from(bindings);
			}
		}
		if !overrides.is_empty() {
			return Err(format!(
				"Layout {:?} overrides an unknown Input Action",
				layout.label
			));
		}
		Ok(actions)
	}

	/// Display names can change without invalidating a saved local layout.
	pub fn binding_digest(&self) -> Result<String, String> {
		self.validate()?;
		let bindings: BTreeMap<_, _> = self
			.actions
			.iter()
			.map(|action| (action.id.as_str(), (action.value, &action.bindings)))
			.collect();
		let payload = serde_json::to_vec(&(self.schema_version, bindings))
			.map_err(|error| error.to_string())?;
		Ok(format!("{:x}", Sha256::digest(payload)))
	}

	pub fn with_layout(&self, id: &AssetId, layout: &InputActionLayout) -> Result<Self, String> {
		if layout.schema_version != 1
			|| &layout.asset_id != id
			|| layout.binding_digest != self.binding_digest()?
			|| layout.overrides.len() > self.actions.len()
		{
			return Err(
				"Local Input Actions layout belongs to a different asset or binding revision"
					.into(),
			);
		}
		let mut result = self.clone();
		for (id, bindings) in &layout.overrides {
			let action = result
				.actions
				.iter_mut()
				.find(|action| &action.id == id)
				.ok_or("Local Input Actions layout has an unknown action")?;
			action.bindings.clone_from(bindings);
		}
		result.validate()?;
		Ok(result)
	}

	pub fn contains_key(&self, key: Key) -> bool {
		self.keys().any(|current| current == key)
	}

	pub fn held_keyboard_codes(&self, keyboard: &InputState) -> Vec<String> {
		self.keys()
			.filter(|&key| keyboard.is_held(key))
			.map(Key::physical_code)
			.collect::<BTreeSet<_>>()
			.into_iter()
			.collect()
	}

	pub(crate) fn keys(&self) -> impl Iterator<Item = Key> + '_ {
		self.actions
			.iter()
			.flat_map(|action| &action.bindings)
			.chain(
				self.layouts
					.iter()
					.flat_map(|layout| &layout.overrides)
					.flat_map(|entry| &entry.bindings),
			)
			.flat_map(|binding| binding.controls().into_iter().flatten())
			.filter_map(|control| match control {
				PhysicalControl::Key(key) => Some(key),
				_ => None,
			})
	}

	pub fn uses_pointer_look(&self) -> bool {
		self.actions.iter().any(|action| {
			action.id == "look" && action.bindings.contains(&ActionBinding::PointerDelta)
		})
	}

	pub fn resolve(
		&self,
		keyboard: &InputState,
		gamepad: PhysicalGamepadSample,
		pointer_delta: [f32; 2],
	) -> ResolvedInputActions {
		self.resolve_from(|key| keyboard.is_held(key), gamepad, pointer_delta)
	}

	pub fn resolve_codes(
		&self,
		held_codes: &HashSet<String>,
		gamepad: PhysicalGamepadSample,
		pointer_delta: [f32; 2],
	) -> ResolvedInputActions {
		self.resolve_from(
			|key| held_codes.contains(&key.physical_code()),
			gamepad,
			pointer_delta,
		)
	}

	fn resolve_from(
		&self,
		is_held: impl Fn(Key) -> bool,
		gamepad: PhysicalGamepadSample,
		pointer_delta: [f32; 2],
	) -> ResolvedInputActions {
		let mut resolved = ResolvedInputActions::default();
		for action in &self.actions {
			match action.value {
				InputActionKind::Button => {
					let held = action.bindings.iter().any(|binding| match binding {
						ActionBinding::Key { key } => is_held(*key),
						ActionBinding::GamepadButton { button } => {
							gamepad.face_buttons[*button as usize]
						}
						_ => false,
					});
					if held {
						resolved.buttons.insert(action.id.clone());
					}
				}
				InputActionKind::Vector2 => {
					let mut finite = [0.0_f32; 2];
					for binding in &action.bindings {
						let sample = match binding {
							ActionBinding::KeyboardVector { keys } => {
								keys.direction_from_held(&is_held)
							}
							ActionBinding::GamepadLeftStick => gamepad.movement(),
							_ => [0.0; 2],
						};
						if sample[0].hypot(sample[1]) > finite[0].hypot(finite[1]) {
							finite = sample;
						}
					}
					resolved.vectors.insert(action.id.clone(), finite);
				}
				InputActionKind::Delta2 => {
					let delta = if action.bindings.contains(&ActionBinding::PointerDelta)
						&& pointer_delta.iter().all(|value| value.is_finite())
					{
						pointer_delta.map(|value| value.clamp(-8.0, 8.0))
					} else {
						[0.0; 2]
					};
					resolved.deltas.insert(action.id.clone(), delta);
					let mut rate = [0.0_f32; 2];
					for binding in &action.bindings {
						let sample = match binding {
							ActionBinding::KeyboardLook { keys } => {
								keys.direction_from_held(&is_held)
							}
							ActionBinding::GamepadRightStick => gamepad.right_stick(),
							_ => [0.0; 2],
						};
						if sample[0].hypot(sample[1]) > rate[0].hypot(rate[1]) {
							rate = sample;
						}
					}
					resolved.look_rates.insert(action.id.clone(), rate);
				}
			}
		}
		resolved
	}
}

fn validate_actions(actions: &[NamedInputAction]) -> Result<(), String> {
	let mut ids = HashSet::new();
	let mut controls = HashMap::new();
	for action in actions {
		if !valid_game_action_id(&action.id) || !ids.insert(action.id.as_str()) {
			return Err(format!(
				"Invalid or duplicate Input Action ID {:?}",
				action.id
			));
		}
		if !valid_display_label(&action.label) {
			return Err(format!(
				"Action {:?} requires a display name of 1–96 bytes",
				action.id
			));
		}
		let expected = match action.id.as_str() {
			"move" => InputActionKind::Vector2,
			"look" => InputActionKind::Delta2,
			"jump" | "attack" | "interact" => InputActionKind::Button,
			_ => action.value,
		};
		if expected != action.value || action.bindings.len() > MAX_BINDINGS {
			return Err(format!(
				"Action {:?} has the wrong value type or more than eight bindings",
				action.id
			));
		}
		for binding in &action.bindings {
			if binding.kind() != action.value {
				return Err(format!(
					"Action {:?} has a binding with the wrong value type",
					action.id
				));
			}
			for control in binding.controls().into_iter().flatten() {
				if matches!(control, PhysicalControl::Key(key) if !supported_key(key)) {
					return Err(format!("Action {:?} uses a reserved control", action.id));
				}
				if let Some(previous) = controls.insert(control, action.id.as_str()) {
					return Err(format!(
						"Actions {previous:?} and {:?} use a conflicting control",
						action.id
					));
				}
			}
		}
	}
	Ok(())
}

fn valid_display_label(label: &str) -> bool {
	label.trim() == label
		&& !label.is_empty()
		&& label.len() <= 96
		&& !label.chars().any(char::is_control)
}

impl ResolvedInputActions {
	/// Legacy layout keeps its original IDs and animation bool parameters.
	pub fn from_legacy(profile: &InputProfile, state: &InputState, gamepad: GamepadSample) -> Self {
		let legacy = profile.resolve_controls(state, gamepad);
		Self {
			buttons: legacy.actions.ids.into_iter().collect(),
			vectors: BTreeMap::from([("move".into(), legacy.movement)]),
			deltas: BTreeMap::new(),
			look_rates: BTreeMap::new(),
			animation_parameters: legacy.actions.animation_parameters.into_iter().collect(),
		}
	}
}
