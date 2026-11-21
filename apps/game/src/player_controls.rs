//! Installed Player Controls and input presentation.

#[cfg(any(windows, test))]
mod controller_menu_input;
mod display;
mod preferences;
mod save_menu;

#[cfg(windows)]
use controller_menu_input::ControllerMenuInput;
use courtyard_gameplay::checkpoint::ContinueAvailability;
#[cfg(windows)]
use runtime::input::PhysicalGamepadSample;
use runtime::{
	Key,
	input::{ControllerBindings, FaceButton, InputProfile, MovementBindings},
};
use serde::{Deserialize, Serialize};
use std::path::Path;

use super::GameApp;
use display::{DisplayAction, DisplayMenu, DisplaySettings};
use preferences::Preferences;
use save_menu::{Intent, SaveMenu};

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
	Controller,
	Display,
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
				Self::Controller,
				Self::Display,
				Self::Back,
			],
			false => &[
				Self::Layout,
				Self::Apply,
				Self::Controller,
				Self::Display,
				Self::Back,
			],
		}
	}

	fn adjacent(self, forward: bool, checkpoints: bool) -> Self {
		let rows = Self::available(checkpoints);
		let index = rows.iter().position(|row| *row == self).unwrap_or_default();
		rows[(index + if forward { 1 } else { rows.len() - 1 }) % rows.len()]
	}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ControlsSettings {
	movement: Preset,
	controller: ControllerBindings,
	display: DisplaySettings,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum PromptDevice {
	#[default]
	Keyboard,
	Controller,
}

impl PromptDevice {
	fn controls_help(self, columns: usize) -> &'static str {
		let choices = match self {
			Self::Keyboard => [
				"Arrows: move  Enter: select  Esc: back",
				"Enter: select  Esc: back",
				"Enter / Esc",
			],
			Self::Controller => [
				"D-pad: move  A: select  B: back",
				"A: select  B: back",
				"A / B",
			],
		};
		choices
			.into_iter()
			.find(|label| label.len() <= columns)
			.unwrap_or(choices[2])
	}
}

enum Page {
	Main,
	Controller {
		focus: usize,
		draft: ControllerBindings,
	},
	Display(DisplayMenu),
	Saves(SaveMenu),
}

struct Menu {
	focus: Row,
	draft: Preset,
	checkpoints: bool,
	page: Page,
}

#[derive(Default)]
pub(super) struct PlayerControls {
	menu: Option<Menu>,
	applied: ControlsSettings,
	prompt_device: PromptDevice,
	menu_device: PromptDevice,
	#[cfg(windows)]
	controller_menu_input: ControllerMenuInput,
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
		let project = project?;
		let settings = Preferences::for_project(project, save_id);
		match settings {
			Ok(preferences) => {
				let choice = preferences.load();
				self.preferences = Some(preferences);
				match choice {
					Ok(settings) => {
						self.applied.controller = settings.controller;
						self.applied.display = settings.display;
						if override_active {
							self.applied.movement = settings.movement;
							return None;
						}
						match settings.movement.resolve(authored) {
							Ok(profile) => {
								self.applied.movement = settings.movement;
								return (settings.movement != Preset::Project).then_some(profile);
							}
							Err(error) => self.message = Some(error),
						}
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

	pub(super) fn controller(&self) -> ControllerBindings {
		self.applied.controller
	}

	pub(super) fn display(&self) -> DisplaySettings {
		self.applied.display
	}

	pub(super) fn prompt_device(&self) -> PromptDevice {
		self.prompt_device
	}

	pub(super) fn set_prompt_device(&mut self, device: PromptDevice) -> bool {
		let previous = std::mem::replace(&mut self.prompt_device, device);
		previous != device
	}

	#[cfg(windows)]
	pub(super) fn reset_controller_menu_input(&mut self) {
		self.controller_menu_input.reset();
	}

	#[cfg(windows)]
	pub(super) fn controller_disconnected(&mut self) {
		self.reset_controller_menu_input();
		self.set_prompt_device(PromptDevice::Keyboard);
		self.menu_device = PromptDevice::Keyboard;
	}

	#[cfg(test)]
	pub(super) fn rows(&self) -> Option<Vec<String>> {
		self.rows_with_capacity(8)
	}

	#[cfg(test)]
	pub(super) fn rows_with_capacity(&self, capacity: usize) -> Option<Vec<String>> {
		self.rows_with_layout(capacity, usize::MAX)
	}

	pub(super) fn rows_with_layout(&self, capacity: usize, columns: usize) -> Option<Vec<String>> {
		let menu = self.menu.as_ref()?;
		let (title, actions, focus) = match &menu.page {
			Page::Controller { focus, draft } => (
				"Controller".into(),
				vec![
					format!("Jump: {:?}", draft.jump),
					format!("Interact: {:?}", draft.interact),
					"Apply".into(),
					"Back".into(),
				],
				*focus,
			),
			Page::Display(display) => display.rows(),
			Page::Saves(saves) => saves.rows(),
			Page::Main => self.main_rows(menu),
		};
		let help = self
			.message
			.as_deref()
			.unwrap_or_else(|| self.menu_device.controls_help(columns));
		Some(focused_rows(title, actions, focus, help, capacity))
	}

	fn main_rows(&self, menu: &Menu) -> (String, Vec<String>, usize) {
		let layout = if self.override_active {
			"Launch profile override"
		} else {
			menu.draft.label()
		};
		let available = Row::available(menu.checkpoints);
		let rows = available
			.iter()
			.map(|row| match row {
				Row::Layout => format!("Movement: {layout}"),
				Row::Apply if self.override_active => "Apply disabled".into(),
				Row::Apply => "Apply".into(),
				Row::Save => "Save checkpoint".into(),
				Row::Continue => "Continue from slot".into(),
				Row::Controller => "Controller".into(),
				Row::Display => "Accessibility".into(),
				Row::Back => "Back".into(),
			})
			.collect();
		(
			"Controls".into(),
			rows,
			available
				.iter()
				.position(|row| *row == menu.focus)
				.unwrap_or_default(),
		)
	}

	fn apply_movement(
		&mut self,
		preset: Preset,
		authored: Option<&InputProfile>,
	) -> Result<InputProfile, String> {
		let profile = preset.resolve(authored)?;
		self.apply_settings(ControlsSettings {
			movement: preset,
			..self.applied
		})?;
		Ok(profile)
	}

	fn apply_controller(&mut self, controller: ControllerBindings) -> Result<(), String> {
		self.apply_settings(ControlsSettings {
			controller,
			..self.applied
		})
	}

	fn apply_display(&mut self, display: DisplaySettings) -> Result<(), String> {
		self.apply_settings(ControlsSettings {
			display,
			..self.applied
		})
	}

	fn apply_settings(&mut self, settings: ControlsSettings) -> Result<(), String> {
		let preferences = self
			.preferences
			.as_ref()
			.ok_or("Local controls storage is unavailable")?;
		preferences.save(settings)?;
		self.applied = settings;
		self.message = None;
		Ok(())
	}
}

impl GameApp {
	pub(super) fn player_controls_key(&mut self, key: Key) -> bool {
		self.handle_player_controls(key, PromptDevice::Keyboard)
	}

	#[cfg(windows)]
	pub(super) fn player_controls_gamepad(&mut self, sample: PhysicalGamepadSample) -> bool {
		let open = self.player_controls.is_open();
		self.player_controls
			.controller_menu_input
			.update(sample, open)
			.is_some_and(|intent| {
				self.handle_player_controls(intent.key(), PromptDevice::Controller)
			})
	}

	fn handle_player_controls(&mut self, key: Key, device: PromptDevice) -> bool {
		match key {
			Key::Escape => {}
			Key::Enter | Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight
			if self.player_controls.is_open() => {}
			_ => return self.player_controls.is_open(),
		}
		self.player_controls.menu_device = device;
		let Some(mut menu) = self.player_controls.menu.take() else {
			let checkpoints = self.checkpoints.is_some();
			self.player_controls.menu = Some(Menu {
				focus: Row::Layout,
				draft: self.player_controls.applied.movement,
				checkpoints,
				page: Page::Main,
			});
			self.clear_controls_input();
			return true;
		};
		let close = match &mut menu.page {
			Page::Main => self.controls_main_key(&mut menu, key),
			Page::Controller { focus, draft } => {
				if self.controller_menu_key(focus, draft, key) {
					menu.page = Page::Main;
				}
				false
			}
			Page::Display(display) => {
				if self.display_menu_key(display, key) {
					menu.page = Page::Main;
				}
				false
			}
			Page::Saves(saves) => {
				if !saves.key(self, key) {
					menu.page = Page::Main;
				}
				false
			}
		};
		if close {
			self.clear_controls_input();
		} else {
			self.player_controls.menu = Some(menu);
		}
		true
	}

	fn controls_main_key(&mut self, menu: &mut Menu, key: Key) -> bool {
		match key {
			Key::Escape | Key::Enter if key == Key::Escape || matches!(menu.focus, Row::Back) => {
				return true;
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
				if self.play_trace.is_some() {
					self.player_controls.message =
						Some("Movement cannot change during playtrace recording".into());
					return false;
				}
				let preset = menu.draft;
				let authored = self
					.project
					.as_ref()
					.and_then(|project| project.input_profile.as_ref());
				match self.player_controls.apply_movement(preset, authored) {
					Ok(profile) => {
						self.editor
							.set_play_input_profile(profile.clone(), &mut self.input);
						self.input_profile_override = Some(profile);
						return true;
					}
					Err(error) => self.player_controls.message = Some(error),
				}
			}
			Key::Enter if matches!(menu.focus, Row::Save) => {
				self.player_controls.message = None;
				menu.page = Page::Saves(SaveMenu::new(self, Intent::Save));
			}
			Key::Enter if matches!(menu.focus, Row::Continue) => {
				self.player_controls.message = None;
				menu.page = Page::Saves(SaveMenu::new(self, Intent::Continue));
			}
			Key::Enter if matches!(menu.focus, Row::Controller) => {
				self.player_controls.message = None;
				menu.page = Page::Controller {
					focus: 0,
					draft: self.player_controls.controller(),
				};
			}
			Key::Enter if matches!(menu.focus, Row::Display) => {
				self.player_controls.message = None;
				menu.page = Page::Display(DisplayMenu::new(self.player_controls.display()));
			}
			_ => {}
		}
		false
	}

	fn display_menu_key(&mut self, menu: &mut DisplayMenu, key: Key) -> bool {
		match menu.key(key) {
			DisplayAction::Back => {
				self.player_controls.message = None;
				true
			}
			DisplayAction::Stay => false,
			DisplayAction::Apply(settings) => match self.player_controls.apply_display(settings) {
				Ok(()) => {
					if !settings.cue_captions {
						self.feedback_captions.clear();
					}
					self.player_controls.message = Some("Display settings saved".into());
					true
				}
				Err(error) => {
					self.player_controls.message = Some(error);
					false
				}
			},
		}
	}

	fn controller_menu_key(
		&mut self,
		focus: &mut usize,
		draft: &mut ControllerBindings,
		key: Key,
	) -> bool {
		match key {
			Key::Escape => return true,
			Key::ArrowUp | Key::ArrowDown => *focus = advance_row(*focus, 4, key),
			Key::ArrowLeft | Key::ArrowRight if *focus < 2 => {
				let button = if *focus == 0 {
					&mut draft.jump
				} else {
					&mut draft.interact
				};
				let step = if key == Key::ArrowRight { 1 } else { 3 };
				*button = FaceButton::ALL[(*button as usize + step) % FaceButton::ALL.len()];
				self.player_controls.message = None;
			}
			Key::Enter if *focus == 2 => match self.player_controls.apply_controller(*draft) {
				Ok(()) => {
					self.clear_controls_input();
					self.player_controls.message = Some("Controller mapping saved".into());
					return true;
				}
				Err(error) => self.player_controls.message = Some(error),
			},
			Key::Enter if *focus == 3 => return true,
			_ => {}
		}
		false
	}

	fn clear_controls_input(&mut self) {
		self.set_mouse_capture(false);
		self.input.suppress_held();
		self.clear_game_controls();
		self.clock.reset();
	}
}

fn focused_rows(
	title: String,
	actions: Vec<String>,
	focus: usize,
	help: &str,
	capacity: usize,
) -> Vec<String> {
	if capacity == 0 || actions.is_empty() {
		return Vec::new();
	}
	let header = capacity >= 3;
	let body_capacity =
		(capacity - usize::from(header) - usize::from(capacity >= 2)).min(actions.len());
	let first = focus.saturating_sub(body_capacity - 1);
	let mut rows = Vec::with_capacity(capacity);
	if header {
		rows.push(title);
	}
	rows.extend(
		actions
			.into_iter()
			.enumerate()
			.skip(first)
			.take(body_capacity)
			.map(|(index, text)| {
				let prefix = if index == focus { "> " } else { "  " };
				format!("{prefix}{text}")
			}),
	);
	if capacity >= 2 {
		rows.push(help.into());
	}
	rows
}

fn advance_row(index: usize, count: usize, key: Key) -> usize {
	(index + if key == Key::ArrowDown { 1 } else { count - 1 }) % count
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
