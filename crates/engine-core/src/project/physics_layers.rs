//! Project names and pair policy for the existing 32 collider membership bits.

mod diagnostics;
pub(crate) use diagnostics::validate_required_physics_contacts;
pub use diagnostics::{PhysicsContactIssue, inspect_physics_contacts};

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const COLLISION_LAYER_COUNT: u8 = u32::BITS as u8;
const MAX_LAYER_ID_BYTES: usize = 32;
const MAX_LAYER_LABEL_BYTES: usize = 80;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicsLayer {
	pub bit: u8,
	pub id: String,
	pub label: String,
	#[serde(default, skip_serializing_if = "is_live")]
	pub archived: bool,
}

fn is_live(archived: &bool) -> bool {
	!*archived
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicsLayers {
	#[serde(default)]
	pub layers: Vec<PhysicsLayer>,
	/// Canonical unordered pairs. Unlisted pairs are allowed, including unnamed bits.
	#[serde(default)]
	pub ignored_pairs: Vec<[u8; 2]>,
}

impl PhysicsLayers {
	pub fn validate(&self) -> Result<(), String> {
		if self.layers.len() > usize::from(COLLISION_LAYER_COUNT) {
			return Err("Physics layers exceed the 32 available bits".into());
		}
		let mut occupied = 0u32;
		let mut ids = HashSet::with_capacity(self.layers.len());
		for layer in &self.layers {
			if layer.bit >= COLLISION_LAYER_COUNT {
				return Err(format!("Physics layer bit {} is outside 0..=31", layer.bit));
			}
			let bit = 1u32 << layer.bit;
			if occupied & bit != 0 {
				return Err(format!("Duplicate physics layer bit {}", layer.bit));
			}
			occupied |= bit;
			let mut id = layer.id.bytes();
			if layer.id.len() > MAX_LAYER_ID_BYTES
				|| !id.next().is_some_and(|byte| byte.is_ascii_lowercase())
				|| !id
					.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
			{
				return Err("Physics layer IDs must begin with a lowercase ASCII letter and contain at most 32 lowercase ASCII letters, digits, or underscores".into());
			}
			if !ids.insert(layer.id.as_str()) {
				return Err(format!("Duplicate physics layer ID {:?}", layer.id));
			}
			if layer.label.trim().is_empty()
				|| layer.label.len() > MAX_LAYER_LABEL_BYTES
				|| layer.label.chars().any(char::is_control)
			{
				return Err(format!(
					"Physics layer {:?} label must contain visible text, at most 80 bytes, and no control characters",
					layer.id
				));
			}
		}
		let count = usize::from(COLLISION_LAYER_COUNT);
		if self.ignored_pairs.len() > count * (count + 1) / 2 {
			return Err("Physics matrix exceeds the number of unordered layer pairs".into());
		}
		let mut seen = [0u32; COLLISION_LAYER_COUNT as usize];
		for &[left, right] in &self.ignored_pairs {
			if left > right || right >= COLLISION_LAYER_COUNT {
				return Err(format!(
					"Physics matrix pair [{left}, {right}] must have ascending bits within 0..=31"
				));
			}
			let bit = 1u32 << right;
			let row = &mut seen[usize::from(left)];
			if *row & bit != 0 {
				return Err(format!("Duplicate ignored physics pair [{left}, {right}]"));
			}
			*row |= bit;
		}
		Ok(())
	}
}
