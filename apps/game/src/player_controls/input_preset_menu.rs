//! Keyboard and controller navigation for portable input presets.

use super::{
	ControlsSettings, GameApp, Key, advance_row,
	input_labels::compact_key_name,
	input_preset::{PresetPreview, PresetTransfer},
	preset_library_menu::LibraryMenu,
};
use crate::local_player_files::{ordinary_transfer, transfer_entries, transfer_label};
use runtime::input::InputProfile;
use std::{
	path::PathBuf,
	time::{SystemTime, UNIX_EPOCH},
};

const SUFFIX: &str = ".input-preset.json";

struct PresetFile {
	label: String,
	path: PathBuf,
}

enum Page {
	Main,
	Files,
	Library(Box<LibraryMenu>),
	Review {
		result: Result<PresetPreview, String>,
		focus: usize,
	},
}

pub(super) enum PresetAction {
	Stay,
	Back,
	Applied,
}

pub(super) struct PresetMenu {
	owner: PresetTransfer,
	apply_blocked: Option<&'static str>,
	page: Page,
	main_focus: usize,
	file_focus: usize,
	files: Vec<PresetFile>,
}

impl PresetMenu {
	pub(super) fn new(app: &GameApp) -> Result<Self, String> {
		let message = "Input presets require an installed project";
		Ok(Self {
			owner: PresetTransfer::new(
				app.project_path.as_deref().ok_or(message)?,
				app.project.as_ref().ok_or(message)?,
			)?,
			apply_blocked: app.input_preset_apply_blocked(),
			page: Page::Main,
			main_focus: 0,
			file_focus: 0,
			files: Vec::new(),
		})
	}

	pub(super) fn rows(&self) -> (String, Vec<String>, usize) {
		match &self.page {
			Page::Main => (
				"Input presets".into(),
				vec![
					"Export local preset".into(),
					"Review preset file".into(),
					"Save Current Layout".into(),
					"Saved Layouts".into(),
					"Back".into(),
				],
				self.main_focus,
			),
			Page::Files => {
				let mut rows: Vec<_> = self.files.iter().map(|file| file.label.clone()).collect();
				let title = if rows.is_empty() {
					"No input preset files"
				} else {
					"Choose input preset"
				};
				rows.push("Back".into());
				(title.into(), rows, self.file_focus)
			}
			Page::Review { result, focus } => {
				let rows = review_rows(result, self.apply_blocked);
				let title = self.files[self.file_focus].label.clone();
				(title, rows, *focus)
			}
			Page::Library(library) => library.rows(self.apply_blocked),
		}
	}

	pub(super) fn key(&mut self, app: &mut GameApp, key: Key) -> PresetAction {
		match &mut self.page {
			Page::Main => match key {
				Key::Escape => return PresetAction::Back,
				Key::ArrowUp | Key::ArrowDown => {
					self.main_focus = advance_row(self.main_focus, 5, key);
				}
				Key::Enter => match self.main_focus {
					0 => {
						app.player_controls.message =
							Some(self.export(app).unwrap_or_else(|error| error));
					}
					1 => match self.list(app) {
						Ok(files) => {
							self.files = files;
							self.file_focus = 0;
							self.page = Page::Files;
							app.player_controls.message = None;
						}
						Err(error) => app.player_controls.message = Some(error),
					},
					2 | 3 => match LibraryMenu::new(&self.owner, self.main_focus == 2) {
						Ok(library) => {
							self.page = Page::Library(Box::new(library));
							app.player_controls.message = None;
						}
						Err(error) => app.player_controls.message = Some(error),
					},
					_ => return PresetAction::Back,
				},
				_ => {}
			},
			Page::Files => match key {
				Key::Escape => self.leave_files(app),
				Key::ArrowUp | Key::ArrowDown => {
					self.file_focus = advance_row(self.file_focus, self.files.len() + 1, key);
					app.player_controls.message = None;
				}
				Key::Enter => {
					if let Some(file) = self.files.get(self.file_focus) {
						let result = self
							.check_file(app, file)
							.and_then(|()| self.owner.preview(&file.path));
						self.page = Page::Review { result, focus: 0 };
						app.player_controls.message = None;
					} else {
						self.leave_files(app);
					}
				}
				_ => {}
			},
			Page::Review { result, focus } => match key {
				Key::Escape => self.leave_review(app),
				Key::ArrowUp | Key::ArrowDown => {
					*focus = advance_row(*focus, review_length(result), key);
				}
				Key::Enter if *focus == 0 => self.leave_review(app),
				Key::Enter if *focus == review_length(result) - 1 => {
					let Ok(preview) = result else {
						return PresetAction::Stay;
					};
					let (source, destination) = (preview.source_sha256, preview.destination_sha256);
					if let Some(reason) = app.input_preset_apply_blocked() {
						app.player_controls.message = Some(reason.into());
						return PresetAction::Stay;
					}
					let file = &self.files[self.file_focus];
					let applied = self
						.check_file(app, file)
						.and_then(|()| self.owner.apply_reviewed(&file.path, source, destination));
					match applied {
						Ok((settings, profile)) => return apply_to_player(app, settings, profile),
						Err(error) => {
							self.page = Page::Review {
								result: Err(error),
								focus: 0,
							}
						}
					}
				}
				_ => {}
			},
			Page::Library(library) => match library.key(app, &self.owner, key) {
				PresetAction::Back => {
					self.page = Page::Main;
					app.player_controls.message = None;
				}
				result => return result,
			},
		}
		PresetAction::Stay
	}

	fn list(&self, app: &GameApp) -> Result<Vec<PresetFile>, String> {
		let Some(folder) = self
			.owner
			.transfer_directory(app.transfer_directory.as_deref(), false)?
		else {
			return Ok(Vec::new());
		};
		Ok(transfer_entries(&folder, SUFFIX)?
			.into_iter()
			.enumerate()
			.map(|(index, (name, path))| PresetFile {
				label: transfer_label("Preset", index + 1, &name, SUFFIX),
				path,
			})
			.collect())
	}

	fn export(&self, app: &GameApp) -> Result<String, String> {
		let stamp = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.map_err(|error| error.to_string())?
			.as_millis();
		let folder = self
			.owner
			.transfer_directory(app.transfer_directory.as_deref(), true)?
			.ok_or("Input preset folder is unavailable")?;
		let filename = format!("input-{stamp}{SUFFIX}");
		self.owner
			.export_applied(&folder.join(&filename), &app.player_controls.applied)?;
		Ok(format!("Exported input-{stamp}"))
	}

	fn check_file(&self, app: &GameApp, file: &PresetFile) -> Result<(), String> {
		let folder = self
			.owner
			.transfer_directory(app.transfer_directory.as_deref(), false)?
			.ok_or("Preset folder is unavailable")?;
		if file.path.parent() != Some(folder.as_path())
			|| ordinary_transfer(&file.path, SUFFIX)?.is_none()
		{
			return Err("Preset file changed. Reopen the file list.".into());
		}
		Ok(())
	}

	fn leave_files(&mut self, app: &mut GameApp) {
		self.page = Page::Main;
		app.player_controls.message = None;
	}

	fn leave_review(&mut self, app: &mut GameApp) {
		self.page = Page::Files;
		app.player_controls.message = None;
	}
}

pub(super) fn apply_to_player(
	app: &mut GameApp,
	settings: ControlsSettings,
	profile: InputProfile,
) -> PresetAction {
	app.player_controls.applied = settings;
	app.player_controls.keyboard_problem = None;
	app.editor
		.set_play_input_profile(profile.clone(), &mut app.input);
	app.input_profile_override = Some(profile);
	app.clear_controls_input();
	app.player_controls.message = Some("Input preset applied".into());
	PresetAction::Applied
}

pub(super) fn review_length(result: &Result<PresetPreview, String>) -> usize {
	result
		.as_ref()
		.map_or(2, |preview| preview.actions.len() + 5)
}

pub(super) fn review_rows(
	result: &Result<PresetPreview, String>,
	disabled: Option<&str>,
) -> Vec<String> {
	let mut rows = vec!["Keep current".into()];
	let Ok(review) = result else {
		if let Err(error) = result {
			rows.push(error.clone());
		}
		return rows;
	};
	rows.extend([
		format!(
			"Movement: {} > {}",
			review.movement.current.label(),
			review.movement.proposed.label()
		),
		format!(
			"Jump: {:?} > {:?}",
			review.controller.current.jump, review.controller.proposed.jump
		),
		format!(
			"Interact: {:?} > {:?}",
			review.controller.current.interact, review.controller.proposed.interact
		),
	]);
	rows.extend(review.actions.iter().map(|action| {
		format!(
			"{}: {} > {}",
			action.id,
			compact_key_name(action.keys.current),
			compact_key_name(action.keys.proposed)
		)
	}));
	rows.push(disabled.unwrap_or("Apply preset").into());
	rows
}
