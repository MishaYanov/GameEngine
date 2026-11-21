//! Installed Player keyboard settings and focus, independent of the authored input profile.

mod preferences;

use courtyard_gameplay::checkpoint::ContinueAvailability;
use runtime::{
	Key,
	input::{InputProfile, MovementBindings},
};
use serde::{Deserialize, Serialize};
use std::path::Path;

use super::GameApp;
use preferences::Preferences;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Preset {
	#[default]
	Project,
	Arrows,
	Wasd,
}

impl Preset {
	fn label(self) -> &'static str {
		match self {
			Self::Project => "Project defaults",
			Self::Arrows => "Arrow keys",
			Self::Wasd => "WASD",
		}
	}

	fn adjacent(self, forward: bool) -> Self {
		let choices = [Self::Project, Self::Arrows, Self::Wasd];
		let index = self as usize;
		choices[(index + if forward { 1 } else { 2 }) % choices.len()]
	}

	fn resolve(self, authored: Option<&InputProfile>) -> Result<InputProfile, String> {
		let mut profile = authored.cloned().unwrap_or_default();
		let previous = profile.movement;
		profile.movement = match self {
			Self::Project => previous,
			Self::Arrows => MovementBindings::ARROWS,
			Self::Wasd => MovementBindings::WASD,
		};
		for action in &profile.actions {
			if profile.movement.contains_key(action.key) && !previous.contains_key(action.key) {
				return Err(format!("Key {:?} is assigned to {}", action.key, action.id));
			}
		}
		validate_profile(&profile)?;
		Ok(profile)
	}
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Row {
	#[default]
	Layout,
	Apply,
	Save,
	Continue,
	Back,
}

impl Row {
	fn available(checkpoints: bool) -> &'static [Self] {
		match checkpoints {
			true => &[
				Self::Layout,
				Self::Apply,
				Self::Save,
				Self::Continue,
				Self::Back,
			],
			false => &[Self::Layout, Self::Apply, Self::Back],
		}
	}

	fn adjacent(self, forward: bool, checkpoints: bool) -> Self {
		let rows = Self::available(checkpoints);
		let index = rows.iter().position(|row| *row == self).unwrap_or_default();
		rows[(index + if forward { 1 } else { rows.len() - 1 }) % rows.len()]
	}
}

struct Menu {
	focus: Row,
	draft: Preset,
	checkpoints: bool,
	continue_availability: ContinueAvailability,
}

#[derive(Default)]
pub(super) struct PlayerControls {
	menu: Option<Menu>,
	applied: Preset,
	preferences: Option<Preferences>,
	override_active: bool,
	message: Option<String>,
}

impl PlayerControls {
	pub(super) fn configure(
		&mut self,
		project: Option<&Path>,
		save_id: Option<&str>,
		authored: Option<&InputProfile>,
		override_active: bool,
	) -> Option<InputProfile> {
		self.override_active = override_active;
		if override_active {
			return None;
		}
		let project = project?;
		let settings = Preferences::for_project(project, save_id);
		match settings {
			Ok(preferences) => {
				let choice = preferences.load().and_then(|preset| {
					let profile = preset.resolve(authored)?;
					Ok((preset, profile))
				});
				self.preferences = Some(preferences);
				match choice {
					Ok((preset, profile)) => {
						self.applied = preset;
						return (preset != Preset::Project).then_some(profile);
					}
					Err(error) => self.message = Some(error),
				}
			}
			Err(error) => self.message = Some(error),
		}
		// A rejected stored preference leaves the original file available.
		None
	}

	pub(super) fn is_open(&self) -> bool {
		self.menu.is_some()
	}

	pub(super) fn rows(&self) -> Option<Vec<String>> {
		let menu = self.menu.as_ref()?;
		let layout = if self.override_active {
			"Launch profile override"
		} else {
			menu.draft.label()
		};
		let mut rows = vec!["Controls".into()];
		for row in Row::available(menu.checkpoints) {
			let name = match row {
				Row::Layout => format!("Movement: {layout}"),
				Row::Apply if self.override_active => "Apply disabled".into(),
				Row::Apply => "Apply".into(),
				Row::Save => "Save checkpoint".into(),
				Row::Continue => {
					format!("Continue ({})", continue_label(menu.continue_availability))
				}
				Row::Back => "Back".into(),
			};
			let focus = if *row == menu.focus { "> " } else { "  " };
			rows.push(format!("{focus}{name}"));
		}
		if let Some(message) = &self.message {
			rows.push(message.clone());
		} else if !self.override_active {
			rows.push(format!("Active: {}", self.applied.label()));
		}
		rows.push("Arrows: navigate  Enter: select  Esc: back".into());
		Some(rows)
	}

	fn apply(
		&mut self,
		preset: Preset,
		authored: Option<&InputProfile>,
	) -> Result<InputProfile, String> {
		let profile = preset.resolve(authored)?;
		let preferences = self
			.preferences
			.as_ref()
			.ok_or("Local controls storage is unavailable")?;
		preferences.save(preset)?;
		self.applied = preset;
		self.message = None;
		self.menu = None;
		Ok(profile)
	}
}

impl GameApp {
	pub(super) fn player_controls_key(&mut self, key: Key) -> bool {
		let Some(menu) = &mut self.player_controls.menu else {
			if key != Key::Escape {
				return false;
			}
			let checkpoints = self.checkpoints.is_some();
			let continue_availability = if checkpoints {
				self.checkpoint_availability()
			} else {
				ContinueAvailability::Empty
			};
			self.player_controls.menu = Some(Menu {
				focus: Row::Layout,
				draft: self.player_controls.applied,
				checkpoints,
				continue_availability,
			});
			self.clear_controls_input();
			return true;
		};
		match key {
			Key::Escape | Key::Enter if key == Key::Escape || matches!(menu.focus, Row::Back) => {
				self.player_controls.menu = None;
				self.clear_controls_input();
			}
			Key::ArrowUp | Key::ArrowDown => {
				menu.focus = menu.focus.adjacent(key == Key::ArrowDown, menu.checkpoints);
			}
			Key::ArrowLeft | Key::ArrowRight
			if !self.player_controls.override_active && matches!(menu.focus, Row::Layout) =>
				{
					menu.draft = menu.draft.adjacent(key == Key::ArrowRight);
				}
			Key::Enter
			if !self.player_controls.override_active && matches!(menu.focus, Row::Apply) =>
				{
					let preset = menu.draft;
					let authored = self
						.project
						.as_ref()
						.and_then(|project| project.input_profile.as_ref());
					match self.player_controls.apply(preset, authored) {
						Ok(profile) => {
							self.editor
								.set_play_input_profile(profile.clone(), &mut self.input);
							self.input_profile_override = Some(profile);
							self.clear_controls_input();
						}
						Err(error) => self.player_controls.message = Some(error),
					}
				}
			Key::Enter if matches!(menu.focus, Row::Save) => {
				let result = self.save_checkpoint();
				self.show_checkpoint_result(result);
			}
			Key::Enter if matches!(menu.focus, Row::Continue) => {
				let availability = self.checkpoint_availability();
				let result = if availability.can_continue() {
					self.continue_checkpoint()
				} else {
					Err(continue_label(availability).into())
				};
				self.show_checkpoint_result(result);
			}
			_ => {}
		}
		true
	}

	fn clear_controls_input(&mut self) {
		self.set_mouse_capture(false);
		self.play_gamepad.reset();
		self.input.suppress_held();
		self.editor.clear_play_input();
		self.clock.reset();
	}

	fn show_checkpoint_result(&mut self, result: Result<String, String>) {
		let availability = self.checkpoint_availability();
		if let Some(menu) = self.player_controls.menu.as_mut() {
			menu.continue_availability = availability;
		}
		self.player_controls.message = Some(result.unwrap_or_else(|error| error));
	}
}

fn continue_label(availability: ContinueAvailability) -> &'static str {
	match availability {
		ContinueAvailability::Empty => "No saved checkpoint",
		ContinueAvailability::Current => "Ready",
		ContinueAvailability::CompatibleEarlierRelease => "Earlier release",
		ContinueAvailability::IncompatibleRelease => "Incompatible with this release",
		ContinueAvailability::Unreadable => "Unreadable save",
	}
}

pub(super) fn validate_profile(profile: &InputProfile) -> Result<(), String> {
	profile.validate()?;
	if profile.movement.contains_key(Key::Escape) {
		return Err("Escape is reserved for leaving game input".into());
	}
	Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/player_controls.rs"]
mod tests;
