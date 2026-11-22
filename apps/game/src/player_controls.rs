//! Installed Player Controls and input presentation.

mod action_keys;
mod audio;
#[cfg(any(windows, test))]
mod controller_menu_input;
mod display;
pub(super) mod input_labels;
pub(crate) mod input_preset;
mod input_preset_menu;
mod keyboard_menu;
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
use action_keys::ActionKeyOverride;
use audio::{AudioAction, AudioMenu, FeedbackMix};
use display::{DisplayAction, DisplayMenu, DisplaySettings};
use input_preset_menu::{PresetAction, PresetMenu};
use keyboard_menu::{KeyboardAction, KeyboardMenu};
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

	fn movement(self, default: MovementBindings) -> MovementBindings {
		match self {
			Self::Project => default,
			Self::Arrows => MovementBindings::ARROWS,
			Self::Wasd => MovementBindings::WASD,
		}
	}

	#[cfg(test)]
	fn resolve(self, authored: Option<&InputProfile>) -> Result<InputProfile, String> {
		ControlsSettings {
			movement: self,
			..ControlsSettings::default()
		}
		.resolve_keyboard(authored)
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
	Keyboard,
	Presets,
	Audio,
	Display,
	Back,
}

impl Row {
	fn available(
		checkpoints: bool,
		presets: bool,
	) -> impl DoubleEndedIterator<Item = Self> + Clone {
		[
			Self::Layout,
			Self::Apply,
			Self::Save,
			Self::Continue,
			Self::Controller,
			Self::Keyboard,
			Self::Presets,
			Self::Audio,
			Self::Display,
			Self::Back,
		]
		.into_iter()
		.filter(move |row| match row {
			Self::Save | Self::Continue => checkpoints,
			Self::Presets => presets,
			_ => true,
		})
	}

	fn adjacent(self, forward: bool, checkpoints: bool, presets: bool) -> Self {
		let mut rows = Self::available(checkpoints, presets);
		if forward {
			let first = rows.clone().next().unwrap_or_default();
			rows.find(|row| *row == self)
				.and_then(|_| rows.next())
				.unwrap_or(first)
		} else {
			let last = rows.clone().next_back().unwrap_or_default();
			rows.rfind(|row| *row == self)
				.and_then(|_| rows.next_back())
				.unwrap_or(last)
		}
	}
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ControlsSettings {
	movement: Preset,
	controller: ControllerBindings,
	display: DisplaySettings,
	actions: Vec<ActionKeyOverride>,
	audio: FeedbackMix,
}

impl ControlsSettings {
	fn resolve_keyboard(&self, authored: Option<&InputProfile>) -> Result<InputProfile, String> {
		let fallback = InputProfile::default();
		let authored = authored.unwrap_or(&fallback);
		action_keys::resolve(
			authored,
			self.movement.movement(authored.movement),
			&self.actions,
		)
	}
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
	Keyboard(KeyboardMenu),
	Presets(Box<PresetMenu>),
	Audio(AudioMenu),
	Saves(SaveMenu),
}

struct Menu {
	focus: Row,
	draft: Preset,
	checkpoints: bool,
	presets: bool,
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
	keyboard_problem: Option<String>,
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
					Ok(settings) => return self.restore_settings(settings, authored),
					Err(error) => self.message = Some(error),
				}
			}
			Err(error) => self.message = Some(error),
		}
		// A rejected stored preference leaves the original file available.
		None
	}

	fn restore_settings(
		&mut self,
		settings: ControlsSettings,
		authored: Option<&InputProfile>,
	) -> Option<InputProfile> {
		self.applied = settings;
		self.keyboard_problem = None;
		if self.override_active {
			return None;
		}
		match self.applied.resolve_keyboard(authored) {
			Ok(profile) => {
				let defaults = InputProfile::default();
				(&profile != authored.unwrap_or(&defaults)).then_some(profile)
			}
			Err(error) => {
				self.message = Some("Keyboard uses project defaults. Check Keyboard.".into());
				self.keyboard_problem = Some(error);
				None
			}
		}
	}

	fn captures_action_key(&self) -> bool {
		matches!(
			self.menu.as_ref().map(|menu| &menu.page),
			Some(Page::Keyboard(menu)) if menu.captures_key()
		)
	}

	pub(super) fn cancel_action_key_capture(&mut self) {
		if let Some(Menu {
			page: Page::Keyboard(menu),
			..
		}) = self.menu.as_mut()
			&& menu.cancel_capture()
		{
			self.message = Some("Key capture cancelled".into());
		}
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

	pub(super) fn feedback_mix(&self) -> FeedbackMix {
		self.applied.audio
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
			Page::Keyboard(keyboard) => keyboard.rows(columns),
			Page::Presets(presets) => presets.rows(),
			Page::Audio(audio) => audio.rows(),
			Page::Saves(saves) => saves.rows(),
			Page::Main => self.main_rows(menu),
		};
		let help = if self.captures_action_key() {
			if columns >= 25 {
				"Press a key. Esc/B cancels"
			} else {
				"Esc/B cancels"
			}
		} else {
			self.message
				.as_deref()
				.unwrap_or_else(|| self.menu_device.controls_help(columns))
		};
		Some(focused_rows(title, actions, focus, help, capacity))
	}

	fn main_rows(&self, menu: &Menu) -> (String, Vec<String>, usize) {
		let layout = if self.override_active {
			"Launch profile override"
		} else {
			menu.draft.label()
		};
		let available = Row::available(menu.checkpoints, menu.presets);
		let rows = available
			.clone()
			.map(|row| match row {
				Row::Layout => format!("Movement: {layout}"),
				Row::Apply if self.override_active => "Apply disabled".into(),
				Row::Apply => "Apply".into(),
				Row::Save => "Save checkpoint".into(),
				Row::Continue => "Continue from slot".into(),
				Row::Controller => "Controller".into(),
				Row::Keyboard => "Keyboard".into(),
				Row::Presets => "Input presets".into(),
				Row::Display => "Accessibility".into(),
				Row::Audio => "Feedback Audio".into(),
				Row::Back => "Back".into(),
			})
			.collect();
		(
			"Controls".into(),
			rows,
			available
				.clone()
				.position(|row| row == menu.focus)
				.unwrap_or_default(),
		)
	}

	fn apply_movement(
		&mut self,
		preset: Preset,
		authored: Option<&InputProfile>,
	) -> Result<InputProfile, String> {
		self.apply_keyboard_settings(
			ControlsSettings {
				movement: preset,
				..self.applied.clone()
			},
			authored,
		)
	}

	fn apply_keyboard_settings(
		&mut self,
		settings: ControlsSettings,
		authored: Option<&InputProfile>,
	) -> Result<InputProfile, String> {
		let profile = settings.resolve_keyboard(authored)?;
		self.apply_settings(settings)?;
		self.keyboard_problem = None;
		Ok(profile)
	}

	fn apply_controller(&mut self, controller: ControllerBindings) -> Result<(), String> {
		self.apply_settings(ControlsSettings {
			controller,
			..self.applied.clone()
		})
	}

	fn apply_display(&mut self, display: DisplaySettings) -> Result<(), String> {
		self.apply_settings(ControlsSettings {
			display,
			..self.applied.clone()
		})
	}

	fn apply_audio(&mut self, audio: FeedbackMix) -> Result<(), String> {
		self.apply_settings(ControlsSettings {
			audio: audio.validate()?,
			..self.applied.clone()
		})
	}

	fn apply_settings(&mut self, settings: ControlsSettings) -> Result<(), String> {
		let preferences = self
			.preferences
			.as_ref()
			.ok_or("Local controls storage is unavailable")?;
		preferences.save_if_current(&self.applied, &settings)?;
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
		if self.player_controls.captures_action_key() {
			if device == PromptDevice::Controller && key != Key::Escape {
				return false;
			}
			if device == PromptDevice::Keyboard {
				self.input.press(key);
				self.input.suppress_held();
			}
		} else {
			match key {
				Key::Escape => {}
				Key::Enter | Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight
					if self.player_controls.is_open() => {}
				_ => return self.player_controls.is_open(),
			}
		}
		self.player_controls.menu_device = device;
		let Some(mut menu) = self.player_controls.menu.take() else {
			let checkpoints = self.checkpoints.is_some();
			self.player_controls.menu = Some(Menu {
				focus: Row::Layout,
				draft: self.player_controls.applied.movement,
				checkpoints,
				presets: self.installed_project && self.project_path.is_some(),
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
			Page::Keyboard(keyboard) => {
				if self.keyboard_menu_key(keyboard, key, device) {
					menu.page = Page::Main;
				}
				false
			}
			Page::Presets(presets) => {
				match presets.key(self, key) {
					PresetAction::Stay => {}
					PresetAction::Back => {
						self.player_controls.message = None;
						menu.page = Page::Main;
					}
					PresetAction::Applied => {
						menu.draft = self.player_controls.applied.movement;
						menu.page = Page::Main;
					}
				}
				false
			}
			Page::Display(display) => {
				if self.display_menu_key(display, key) {
					menu.page = Page::Main;
				}
				false
			}
			Page::Audio(audio) => {
				if self.audio_menu_key(audio, key) {
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
				menu.focus =
					menu.focus
						.adjacent(key == Key::ArrowDown, menu.checkpoints, menu.presets);
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
			Key::Enter if matches!(menu.focus, Row::Keyboard) => {
				let editable = !self.player_controls.override_active;
				let keyboard = if editable {
					KeyboardMenu::new(
						self.project
							.as_ref()
							.and_then(|project| project.input_profile.as_ref()),
						&self.player_controls.applied.actions,
						true,
					)
				} else {
					KeyboardMenu::new(self.effective_input_profile(), &[], false)
				};
				self.player_controls.message = if editable {
					self.player_controls.keyboard_problem.clone()
				} else {
					Some("Launch profile override is active".into())
				};
				menu.page = Page::Keyboard(keyboard);
			}
			Key::Enter if matches!(menu.focus, Row::Presets) => match PresetMenu::new(self) {
				Ok(presets) => {
					self.player_controls.message = None;
					menu.page = Page::Presets(Box::new(presets));
				}
				Err(error) => self.player_controls.message = Some(error),
			},
			Key::Enter if matches!(menu.focus, Row::Display) => {
				self.player_controls.message = None;
				menu.page = Page::Display(DisplayMenu::new(self.player_controls.display()));
			}
			Key::Enter if matches!(menu.focus, Row::Audio) => {
				self.player_controls.message = None;
				menu.page = Page::Audio(AudioMenu::new(self.player_controls.feedback_mix()));
			}
			_ => {}
		}
		false
	}

	fn keyboard_menu_key(
		&mut self,
		menu: &mut KeyboardMenu,
		key: Key,
		device: PromptDevice,
	) -> bool {
		match menu.key(key, device) {
			KeyboardAction::Stay | KeyboardAction::Started => {}
			KeyboardAction::Cancelled => {
				self.player_controls.message = Some("Key capture cancelled".into());
			}
			KeyboardAction::Back => {
				self.player_controls.message = None;
				return true;
			}
			KeyboardAction::Locked => {
				self.player_controls.message = Some("Launch profile override is active".into());
			}
			KeyboardAction::Apply if self.play_trace.is_some() => {
				self.player_controls.message =
					Some("Keyboard cannot change during playtrace recording".into());
			}
			action @ (KeyboardAction::Edited | KeyboardAction::Apply) => {
				let settings = ControlsSettings {
					actions: menu.draft().to_vec(),
					..self.player_controls.applied.clone()
				};
				let authored = self
					.project
					.as_ref()
					.and_then(|project| project.input_profile.as_ref());
				if matches!(action, KeyboardAction::Edited) {
					self.player_controls.message = settings.resolve_keyboard(authored).err();
				} else {
					match self
						.player_controls
						.apply_keyboard_settings(settings, authored)
					{
						Ok(profile) => {
							self.editor
								.set_play_input_profile(profile.clone(), &mut self.input);
							self.input_profile_override = Some(profile);
							self.clear_controls_input();
							self.player_controls.message = Some("Keyboard mapping saved".into());
							return true;
						}
						Err(error) => self.player_controls.message = Some(error),
					}
				}
			}
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

	fn audio_menu_key(&mut self, menu: &mut AudioMenu, key: Key) -> bool {
		match menu.key(key) {
			AudioAction::Back => {
				self.player_controls.message = None;
				true
			}
			AudioAction::Stay => false,
			AudioAction::Apply(mix) => {
				// The running mix changes only once the matching local preferences are saved.
				match self
					.feedback_audio
					.prepare_mix(mix.effective_percent())
					.and_then(|prepared| {
						self.player_controls.apply_audio(mix)?;
						Ok(prepared)
					}) {
					Ok(prepared) => {
						self.feedback_audio.commit_mix(prepared);
						self.player_controls.message = Some("Feedback audio saved".into());
						true
					}
					Err(error) => {
						self.player_controls.message = Some(error);
						false
					}
				}
			}
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

	fn input_preset_apply_blocked(&self) -> Option<&'static str> {
		if self.play_trace.is_some() {
			Some("Playtrace recording: Apply disabled")
		} else if self.player_controls.override_active {
			Some("Launch profile override: Apply disabled")
		} else {
			None
		}
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
