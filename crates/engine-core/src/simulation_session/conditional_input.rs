//! Fixed-step selection and bounded raw samples for conditional Input Actions.

use crate::{
	input::{
		InputActionMap, InputLayoutCondition, InputState, Key, PhysicalGamepadSample,
		ResolvedInputActions,
	},
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
	physical_keys: Vec<(Key, String)>,
	last_sample: PhysicalActionSample,
	pending: VecDeque<PhysicalActionSample>,
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
			physical_keys,
			last_sample: PhysicalActionSample::default(),
			pending: VecDeque::new(),
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
		self.pending.clear();
		self.last_sample = PhysicalActionSample::default();
	}

	pub(super) fn has_pending(&self) -> bool {
		!self.pending.is_empty()
	}

	pub(super) fn step(
		&mut self,
		scene: &SceneId,
		grounded: Option<bool>,
	) -> (bool, Vec<ResolvedInputActions>) {
		let active = self
			.layouts
			.iter()
			.position(|layout| layout.condition.matches(scene, grounded));
		let changed = std::mem::replace(&mut self.active, active) != active;
		let mut frames = Vec::with_capacity(self.pending.len() + usize::from(changed));
		if changed {
			frames.push(self.resolve(&self.last_sample));
		}
		while let Some(sample) = self.pending.pop_front() {
			frames.push(self.resolve(&sample));
			self.last_sample = sample;
		}
		(changed, frames)
	}

	fn resolve(&self, sample: &PhysicalActionSample) -> ResolvedInputActions {
		self.current_map()
			.resolve_codes(&sample.keys, sample.pad, [0.0; 2])
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
