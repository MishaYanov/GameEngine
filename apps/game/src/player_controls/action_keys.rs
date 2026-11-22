//! Resolution of a local key draft against the installed project's authored actions.

use std::collections::HashSet;

use runtime::{
	Key,
	input::{InputProfile, MAX_GAME_INPUT_ACTIONS, MovementBindings, valid_game_action_id},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ActionKeyOverride {
	pub action_id: String,
	pub key: Key,
}

pub(super) fn validate_rows(overrides: &[ActionKeyOverride]) -> Result<(), String> {
	if overrides.len() > MAX_GAME_INPUT_ACTIONS {
		return Err(format!(
			"Saved controls allow at most {MAX_GAME_INPUT_ACTIONS} action keys"
		));
	}
	let mut edited = HashSet::with_capacity(overrides.len());
	for row in overrides {
		if !valid_game_action_id(&row.action_id) {
			return Err("Saved controls contain an invalid action ID".into());
		}
		if !edited.insert(row.action_id.as_str()) {
			return Err(format!("Duplicate saved action {}", row.action_id));
		}
		if !can_remap_key(row.key) {
			return Err("Choose a named gameplay key. Esc cancels.".into());
		}
	}
	Ok(())
}

pub(super) fn can_remap_key(key: Key) -> bool {
	!matches!(key, Key::Escape | Key::Unidentified(_))
}

pub(super) fn resolve(
	authored: &InputProfile,
	movement: MovementBindings,
	overrides: &[ActionKeyOverride],
) -> Result<InputProfile, String> {
	validate_rows(overrides)?;
	let mut profile = authored.clone();
	profile.movement = movement;
	for row in overrides {
		let binding = profile
			.actions
			.iter_mut()
			.find(|binding| binding.id == row.action_id)
			.ok_or_else(|| {
				format!(
					"Saved action {} is missing. Restore defaults.",
					row.action_id
				)
			})?;
		binding.key = row.key;
	}
	for (index, (action, original)) in profile.actions.iter().zip(&authored.actions).enumerate() {
		let changed = action.key != original.key;
		let movement_conflict = profile.movement.contains_key(action.key)
			&& (changed || !authored.movement.contains_key(action.key));
		let action_conflict = changed
			&& profile
			.actions
			.iter()
			.enumerate()
			.any(|(other, binding)| other != index && binding.key == action.key);
		if movement_conflict || action_conflict {
			return Err(format!("{}: Key already assigned", action.id));
		}
	}
	super::validate_profile(&profile)?;
	Ok(profile)
}
