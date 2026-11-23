//! Fixed-step selection and bounded raw samples for conditional Input Actions.

use crate::{
	input::{
		ActionBinding, InputActionMap, InputLayoutCondition, InputState, Key,
		PhysicalGamepadSample, ResolvedInputActions,
	},
	runtime_lifecycle::RuntimeEntityHandle,
	scene_catalog::SceneId,
};
use std::collections::{HashSet, VecDeque};

const MAX_PHYSICAL_ACTION_SAMPLES: usize = 128;

#[derive(Default, PartialEq, Eq)]
struct PhysicalActionSample {
	keys: HashSet<String>,
	pad: PhysicalGamepadSample,
}

struct PreparedLayout {
	condition: InputLayoutCondition,
	id: String,
	map: InputActionMap,
}

pub(super) struct ConditionalInputs {
	default: InputActionMap,
	layouts: Vec<PreparedLayout>,
	active: Option<usize>,
	owner: Option<Option<RuntimeEntityHandle>>,
	physical_keys: Vec<(Key, String)>,
	last_sample: PhysicalActionSample,
	pending: VecDeque<PhysicalActionSample>,
	muted_keys: HashSet<String>,
	muted_buttons: [bool; 4],
}

impl ConditionalInputs {
	pub(super) fn new(map: &InputActionMap) -> Result<Self, String> {
		let layouts = map
			.layouts
			.iter()
			.map(|layout| {
				Ok(PreparedLayout {
					condition: layout.when.clone(),
					id: layout.id.clone(),
					map: InputActionMap {
						schema_version: 1,
						actions: map.effective_actions(layout)?,
						layouts: Vec::new(),
					},
				})
			})
			.collect::<Result<Vec<_>, String>>()?;
		let physical_keys = map
			.keys()
			.map(|key| (key, key.physical_code()))
			.collect::<HashSet<_>>()
			.into_iter()
			.collect();
		Ok(Self {
			default: InputActionMap {
				schema_version: 1,
				actions: map.actions.clone(),
				layouts: Vec::new(),
			},
			layouts,
			active: None,
			owner: None,
			physical_keys,
			last_sample: PhysicalActionSample::default(),
			pending: VecDeque::new(),
			muted_keys: HashSet::new(),
			muted_buttons: [false; 4],
		})
	}

	pub(super) fn current_map(&self) -> &InputActionMap {
		self.active
			.map_or(&self.default, |index| &self.layouts[index].map)
	}

	pub(super) fn active_id(&self) -> &str {
		self.active
			.map_or("default", |index| self.layouts[index].id.as_str())
	}

	pub(super) fn sample_keys(
		&mut self,
		keyboard: &InputState,
		pad: PhysicalGamepadSample,
	) -> Result<(), String> {
		self.sample_where(|key, _| keyboard.is_held(key), pad)
	}

	pub(super) fn sample_codes(
		&mut self,
		keyboard: &HashSet<String>,
		pad: PhysicalGamepadSample,
	) -> Result<(), String> {
		self.sample_where(|_, code| keyboard.contains(code), pad)
	}

	pub(super) fn clear(&mut self) {
		if let Some(latest) = self.pending.pop_back() {
			self.last_sample = latest;
		}
		self.muted_keys.clone_from(&self.last_sample.keys);
		self.muted_buttons = self.last_sample.pad.face_buttons;
		self.pending.clear();
	}

	pub(super) fn has_pending(&self) -> bool {
		!self.pending.is_empty()
	}

	pub(super) fn step(
		&mut self,
		scene: &SceneId,
		grounded: Option<bool>,
		owner: Option<RuntimeEntityHandle>,
	) -> (bool, Vec<ResolvedInputActions>) {
		let has_owner = owner.is_some();
		let owner_changed = self
			.owner
			.replace(owner.clone())
			.is_some_and(|old| old != owner);
		if owner_changed {
			self.clear();
		}
		let active = self
			.layouts
			.iter()
			.position(|layout| layout.condition.matches(scene, grounded));
		let changed = self.active != active;
		if changed {
			self.mute_remapped_buttons(active);
			self.active = active;
		}
		let mut frames = Vec::with_capacity(self.pending.len() + usize::from(changed));
		if owner_changed {
			frames.push(ResolvedInputActions::default());
		} else if changed && has_owner {
			frames.push(self.resolve(&self.last_sample));
		}
		while let Some(sample) = self.pending.pop_front() {
			self.muted_keys.retain(|key| sample.keys.contains(key));
			for (muted, held) in self.muted_buttons.iter_mut().zip(sample.pad.face_buttons) {
				*muted &= held;
			}
			if has_owner {
				frames.push(self.resolve(&sample));
			}
			self.last_sample = sample;
		}
		(changed || owner_changed, frames)
	}

	fn mute_remapped_buttons(&mut self, active: Option<usize>) {
		let layout_map = |index: usize| &self.layouts[index].map;
		let previous = self.active.map_or(&self.default, layout_map);
		let next = active.map_or(&self.default, layout_map);
		for (key, code) in &self.physical_keys {
			if self.last_sample.keys.contains(code)
				&& changed_button(
					previous,
					next,
					|binding| matches!(binding, ActionBinding::Key { key: bound } if bound == key),
				) {
				self.muted_keys.insert(code.clone());
			}
		}
		for (index, held) in self.last_sample.pad.face_buttons.into_iter().enumerate() {
			if held
				&& changed_button(
					previous,
					next,
					|binding| matches!(binding, ActionBinding::GamepadButton { button } if *button as usize == index),
				) {
				self.muted_buttons[index] = true;
			}
		}
	}

	fn resolve(&self, sample: &PhysicalActionSample) -> ResolvedInputActions {
		let mut pad = sample.pad;
		for (held, muted) in pad.face_buttons.iter_mut().zip(self.muted_buttons) {
			*held &= !muted;
		}
		let filtered;
		let keys = if self.muted_keys.is_empty() {
			&sample.keys
		} else {
			filtered = sample.keys.difference(&self.muted_keys).cloned().collect();
			&filtered
		};
		self.current_map().resolve_codes(keys, pad, [0.0; 2])
	}

	fn sample_where(
		&mut self,
		held: impl Fn(Key, &str) -> bool,
		pad: PhysicalGamepadSample,
	) -> Result<(), String> {
		let next = PhysicalActionSample {
			keys: self
				.physical_keys
				.iter()
				.filter(|(key, code)| held(*key, code))
				.map(|(_, code)| code.clone())
				.collect(),
			pad,
		};
		if self.pending.back().unwrap_or(&self.last_sample) == &next {
			return Ok(());
		}
		if self.pending.len() == MAX_PHYSICAL_ACTION_SAMPLES {
			return Err("Conditional Input Actions sample budget exceeded".into());
		}
		self.pending.push_back(next);
		Ok(())
	}
}

fn changed_button(
	previous: &InputActionMap,
	next: &InputActionMap,
	bound: impl Fn(&ActionBinding) -> bool,
) -> bool {
	fn action_for(map: &InputActionMap, bound: impl Fn(&ActionBinding) -> bool) -> Option<&str> {
		map.actions
			.iter()
			.find(|action| action.bindings.iter().any(&bound))
			.map(|action| action.id.as_str())
	}
	let before = action_for(previous, &bound);
	let after = action_for(next, bound);
	after.is_some() && before != after
}
