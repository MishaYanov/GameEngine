//! Asset-scoped keyboard choices for a Player using authored Input Actions.

use super::{action_keys::ActionKeyOverride, keyboard_menu::KeyboardMenu};
use runtime::{
	Key,
	asset_catalog::AssetId,
	input::{
		ActionBinding, GameInputBinding, InputActionLayout, InputActionMap, InputProfile,
		NamedInputAction,
	},
};
use std::collections::BTreeMap;

pub(super) struct AuthoredKeyboard {
	pub asset: AssetId,
	pub map: InputActionMap,
	pub stored: Option<InputActionLayout>,
}

impl AuthoredKeyboard {
	pub(super) fn prepare(
		&self,
		rows: &[ActionKeyOverride],
	) -> Result<(Option<InputActionLayout>, InputActionMap), String> {
		if rows.is_empty() {
			return Ok((None, self.map.clone()));
		}
		let mut overrides = BTreeMap::new();
		for row in rows {
			let action = self
				.map
				.actions
				.iter()
				.find(|action| action.id == row.action_id)
				.ok_or("The rebind targets an unavailable action")?;
			let mut bindings = action.bindings.clone();
			let key = bindings
				.iter_mut()
				.find_map(|binding| match binding {
					ActionBinding::Key { key } => Some(key),
					_ => None,
				})
				.ok_or("The selected action has no Default keyboard binding")?;
			*key = row.key;
			overrides.insert(row.action_id.clone(), bindings);
		}
		let layout = InputActionLayout {
			schema_version: 1,
			asset_id: self.asset.clone(),
			binding_digest: self.map.binding_digest()?,
			overrides,
		};
		let effective = self.map.with_layout(&self.asset, &layout)?;
		Ok((Some(layout), effective))
	}
}

pub(super) fn keyboard_menu(
	asset: AssetId,
	map: InputActionMap,
	stored: Result<Option<InputActionLayout>, String>,
	editable: bool,
) -> (KeyboardMenu, Option<String>) {
	let profile = InputProfile {
		actions: map
			.actions
			.iter()
			.filter_map(|action| {
				Some(GameInputBinding {
					id: action.id.clone(),
					key: first_key(action)?,
					animation_parameter: None,
				})
			})
			.collect(),
		..InputProfile::default()
	};
	let mut draft = Vec::new();
	let mut problem = None;
	let stored = match stored {
		Ok(Some(layout)) => {
			match map.with_layout(&asset, &layout) {
				Ok(effective) => {
					for (base, effective) in map.actions.iter().zip(&effective.actions) {
						if let (Some(old), Some(new)) = (first_key(base), first_key(effective)) {
							if old != new {
								draft.push(ActionKeyOverride {
									action_id: base.id.clone(),
									key: new,
								});
							}
						}
					}
				}
				Err(error) => problem = Some(format!("Using authored keys. {error}")),
			}
			Some(layout)
		}
		Ok(None) => None,
		Err(error) => {
			problem = Some(format!("Using authored keys. {error}"));
			None
		}
	};
	let menu = KeyboardMenu::new(Some(&profile), &draft, editable).with_asset(AuthoredKeyboard {
		asset,
		map,
		stored,
	});
	(menu, problem)
}

fn first_key(action: &NamedInputAction) -> Option<Key> {
	action.bindings.iter().find_map(|binding| match binding {
		ActionBinding::Key { key } => Some(*key),
		_ => None,
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use runtime::input::{ConditionalInputLayout, InputBindingOverride, InputLayoutCondition};

	#[test]
	fn keyboard_reports_layout_conflicts_and_uses_defaults_when_saved_digest_is_stale() {
		let asset = AssetId::generate();
		let mut map = InputActionMap::starter();
		map.schema_version = 2;
		map.layouts.push(ConditionalInputLayout {
			id: "land".into(),
			label: "Land".into(),
			when: InputLayoutCondition::CharacterGrounded { value: true },
			overrides: vec![InputBindingOverride {
				action_id: "attack".into(),
				bindings: vec![ActionBinding::Key { key: Key::Q }],
			}],
		});
		let (menu, _) = keyboard_menu(asset.clone(), map.clone(), Ok(None), true);
		let rebind = |key| {
			[ActionKeyOverride {
				action_id: "jump".into(),
				key,
			}]
		};
		let settings = menu.asset().unwrap();
		let error = settings.prepare(&rebind(Key::Q)).unwrap_err();
		for text in ["Land", "jump", "attack"] {
			assert!(error.contains(text), "{error}");
		}
		let (layout, effective) = settings.prepare(&rebind(Key::E)).unwrap();
		let mut layout = layout.unwrap();
		assert_eq!(
			effective.actions[2].bindings.len(),
			map.actions[2].bindings.len()
		);
		assert_eq!(effective.actions[2].bindings[1], map.actions[2].bindings[1]);
		let (menu, problem) =
			keyboard_menu(asset.clone(), map.clone(), Ok(Some(layout.clone())), true);
		assert!(problem.is_none());
		assert_eq!(menu.draft(), &rebind(Key::E));
		layout.binding_digest = "stale".into();
		let (menu, problem) = keyboard_menu(asset, map, Ok(Some(layout)), true);
		assert!(menu.draft().is_empty());
		assert!(problem.unwrap().contains("binding revision"));
	}
}
