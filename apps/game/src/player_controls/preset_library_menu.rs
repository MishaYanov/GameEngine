//! Named input layouts using the same keyboard and controller menu owner.

use super::{
	GameApp, Key, advance_row,
	input_preset::{PresetPreview, PresetTransfer},
	input_preset_menu::{PresetAction, apply_to_player, review_length, review_rows},
	preset_library::{
		BACKUP_SUFFIX, LibraryBackupReview, MAX_LABEL_BYTES, OrganizeLayout, SavedLayout,
	},
};
use crate::local_player_files::{transfer_entries, transfer_label};
use std::{
	path::{Path, PathBuf},
	time::{SystemTime, UNIX_EPOCH},
};

const LABEL_CHARACTERS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 -";

enum Page {
	List,
	Actions(usize),
	Name(NameEditor),
	Review(Result<PresetPreview, String>, usize),
	Delete(usize),
	Backups,
	BackupReview(Result<LibraryBackupReview, String>, RestorePage),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LayoutAction {
	Review,
	Rename,
	Delete,
	SetFavorite(bool),
	MoveEarlier,
	MoveLater,
	Back,
}

impl LayoutAction {
	fn label(self) -> &'static str {
		match self {
			Self::Review => "Review layout",
			Self::Rename => "Rename layout",
			Self::Delete => "Delete layout",
			Self::SetFavorite(true) => "Favorite layout",
			Self::SetFavorite(false) => "Remove favorite",
			Self::MoveEarlier => "Move earlier",
			Self::MoveLater => "Move later",
			Self::Back => "Back",
		}
	}
}

fn layout_actions(layouts: &[SavedLayout], index: usize) -> Vec<LayoutAction> {
	let favorite = layouts[index].favorite;
	let mut actions = vec![
		LayoutAction::Review,
		LayoutAction::Rename,
		LayoutAction::Delete,
		LayoutAction::SetFavorite(!favorite),
	];
	if index > 0 && layouts[index - 1].favorite == favorite {
		actions.push(LayoutAction::MoveEarlier);
	}
	if layouts
		.get(index + 1)
		.is_some_and(|next| next.favorite == favorite)
	{
		actions.push(LayoutAction::MoveLater);
	}
	actions.push(LayoutAction::Back);
	actions
}

enum RestorePage {
	Summary(usize),
	Choices { row: usize, focus: usize },
	Name { row: usize, edit: NameEditor },
}

struct BackupFile {
	label: String,
	path: PathBuf,
}

pub(super) struct LibraryMenu {
	layouts: Vec<SavedLayout>,
	focus: usize,
	page: Page,
	backups: Vec<BackupFile>,
	backup_focus: usize,
}

impl LibraryMenu {
	pub(super) fn new(owner: &PresetTransfer, create: bool) -> Result<Self, String> {
		let layouts = owner.library().layouts()?;
		let page = if create {
			let name = (1..=9)
				.map(|index| format!("LAYOUT {index}"))
				.find(|name| !layouts.iter().any(|row| row.label == *name))
				.ok_or("Choose a name for the layout")?;
			Page::Name(NameEditor::new(name, false))
		} else {
			Page::List
		};
		Ok(Self {
			layouts,
			focus: 0,
			page,
			backups: Vec::new(),
			backup_focus: 0,
		})
	}

	pub(super) fn rows(&self, disabled: Option<&str>) -> (String, Vec<String>, usize) {
		match &self.page {
			Page::List => {
				let title = if self.layouts.is_empty() {
					"No saved layouts"
				} else {
					"Saved Layouts"
				};
				let mut rows: Vec<_> = self
					.layouts
					.iter()
					.map(|entry| {
						if entry.favorite {
							format!("[Favorite] {}", entry.label)
						} else {
							entry.label.clone()
						}
					})
					.collect();
				rows.extend([
					"Export Library".into(),
					"Review Backup".into(),
					"Back".into(),
				]);
				(title.into(), rows, self.focus)
			}
			Page::Actions(focus) => (
				self.layouts[self.focus].label.clone(),
				layout_actions(&self.layouts, self.focus)
					.into_iter()
					.map(|action| action.label().into())
					.collect(),
				*focus,
			),
			Page::Name(edit) => edit.rows(),
			Page::Review(review, focus) => (
				self.layouts[self.focus].label.clone(),
				review_rows(review, disabled),
				*focus,
			),
			Page::Delete(focus) => (
				format!("Delete {}?", self.layouts[self.focus].label),
				vec!["Keep layout".into(), "Delete layout".into()],
				*focus,
			),
			Page::Backups => {
				let title = if self.backups.is_empty() {
					"No library backup files"
				} else {
					"Choose library backup"
				};
				let mut rows: Vec<_> = self.backups.iter().map(|file| file.label.clone()).collect();
				rows.push("Back".into());
				(title.into(), rows, self.backup_focus)
			}
			Page::BackupReview(review, page) => restore_rows(
				&self.backups[self.backup_focus].label,
				review,
				page,
				disabled,
			),
		}
	}

	pub(super) fn key(
		&mut self,
		app: &mut GameApp,
		owner: &PresetTransfer,
		key: Key,
	) -> PresetAction {
		match &mut self.page {
			Page::List => match key {
				Key::Escape => return PresetAction::Back,
				Key::ArrowUp | Key::ArrowDown => {
					self.focus = advance_row(self.focus, self.layouts.len() + 3, key);
					app.player_controls.message = None;
				}
				Key::Enter if self.focus < self.layouts.len() => self.page = Page::Actions(0),
				Key::Enter if self.focus == self.layouts.len() => {
					app.player_controls.message =
						Some(export_library(app, owner).unwrap_or_else(|error| error));
				}
				Key::Enter if self.focus == self.layouts.len() + 1 => {
					match list_backups(app, owner) {
						Ok(backups) => {
							self.backups = backups;
							self.backup_focus = 0;
							self.page = Page::Backups;
							app.player_controls.message = None;
						}
						Err(error) => app.player_controls.message = Some(error),
					}
				}
				Key::Enter => return PresetAction::Back,
				_ => {}
			},
			Page::Actions(focus) => {
				let actions = layout_actions(&self.layouts, self.focus);
				match key {
					Key::Escape => self.page = Page::List,
					Key::ArrowUp | Key::ArrowDown => {
						*focus = advance_row(*focus, actions.len(), key)
					}
					Key::Enter => {
						let action = actions[*focus];
						let layout = &self.layouts[self.focus];
						app.player_controls.message = None;
						match action {
							LayoutAction::Review => {
								self.page = Page::Review(owner.preview_layout(layout), 0)
							}
							LayoutAction::Rename => {
								self.page = Page::Name(NameEditor::new(layout.label.clone(), true))
							}
							LayoutAction::Delete => self.page = Page::Delete(0),
							LayoutAction::Back => self.page = Page::List,
							LayoutAction::SetFavorite(_)
							| LayoutAction::MoveEarlier
							| LayoutAction::MoveLater => {
								let change = match action {
									LayoutAction::SetFavorite(favorite) => {
										OrganizeLayout::SetFavorite(favorite)
									}
									LayoutAction::MoveEarlier => {
										OrganizeLayout::MoveAdjacent(&self.layouts[self.focus - 1])
									}
									LayoutAction::MoveLater => {
										OrganizeLayout::MoveAdjacent(&self.layouts[self.focus + 1])
									}
									_ => unreachable!(),
								};
								let result = owner.library().organize(layout, change);
								let id = layout.id;
								let retain = result.is_ok().then_some(match action {
									LayoutAction::SetFavorite(favorite) => {
										LayoutAction::SetFavorite(!favorite)
									}
									_ => action,
								});
								let message = result
									.map(|()| "Saved layout order".into())
									.unwrap_or_else(|error| error);
								self.refresh(app, owner, &message);
								if let Some(index) =
									self.layouts.iter().position(|item| item.id == id)
								{
									self.focus = index;
									if let Some(action) = retain
										&& let Some(focus) = layout_actions(&self.layouts, index)
										.iter()
										.position(|item| *item == action)
									{
										self.page = Page::Actions(focus);
									}
								}
							}
						}
					}
					_ => {}
				}
			}
			Page::Name(edit) => match edit.key(key) {
				NameAction::Stay => {}
				NameAction::Cancel if edit.rename => self.page = Page::Actions(1),
				NameAction::Cancel => return PresetAction::Back,
				NameAction::Save => {
					let result = if edit.rename {
						owner
							.library()
							.rename(&self.layouts[self.focus], &edit.label)
					} else if app.player_controls.override_active {
						Err("Saving the current layout requires the local input profile".into())
					} else {
						owner.save_layout(&edit.label, &app.player_controls.applied)
					};
					match result {
						Ok(()) => self.refresh(app, owner, "Saved layout"),
						Err(error) => app.player_controls.message = Some(error),
					}
				}
			},
			Page::Review(review, focus) => match key {
				Key::Escape | Key::Enter if key == Key::Escape || *focus == 0 => {
					self.page = Page::Actions(0)
				}
				Key::ArrowUp | Key::ArrowDown => {
					*focus = advance_row(*focus, review_length(review), key)
				}
				Key::Enter if *focus == review_length(review) - 1 => {
					let Ok(review) = review else {
						return PresetAction::Stay;
					};
					if let Some(reason) = app.input_preset_apply_blocked() {
						app.player_controls.message = Some(reason.into());
					} else {
						match owner.apply_layout(&self.layouts[self.focus], review) {
							Ok((settings, profile)) => {
								return apply_to_player(app, settings, profile);
							}
							Err(error) => self.page = Page::Review(Err(error), 0),
						}
					}
				}
				_ => {}
			},
			Page::Delete(focus) => match key {
				Key::Escape | Key::Enter if key == Key::Escape || *focus == 0 => {
					self.page = Page::Actions(2)
				}
				Key::ArrowUp | Key::ArrowDown => *focus = advance_row(*focus, 2, key),
				Key::Enter => match owner.library().delete(&self.layouts[self.focus]) {
					Ok(()) => self.refresh(app, owner, "Deleted layout"),
					Err(error) => app.player_controls.message = Some(error),
				},
				_ => {}
			},
			Page::Backups => match key {
				Key::Escape => self.page = Page::List,
				Key::ArrowUp | Key::ArrowDown => {
					self.backup_focus = advance_row(self.backup_focus, self.backups.len() + 1, key);
				}
				Key::Enter if self.backup_focus == self.backups.len() => self.page = Page::List,
				Key::Enter => {
					let path = &self.backups[self.backup_focus].path;
					let result = check_backup_folder(app, owner, path)
						.and_then(|()| owner.preview_library_backup(path));
					self.page = Page::BackupReview(result, RestorePage::Summary(0));
					app.player_controls.message = None;
				}
				_ => {}
			},
			Page::BackupReview(review, page) => {
				match restore_key(
					review.as_mut().ok(),
					page,
					key,
					&mut app.player_controls.message,
				) {
					RestoreAction::Stay => {}
					RestoreAction::Back => {
						self.page = Page::Backups;
						app.player_controls.message = None;
					}
					RestoreAction::Commit => {
						let Ok(review) = review else {
							return PresetAction::Stay;
						};
						if let Some(reason) = app.input_preset_apply_blocked() {
							app.player_controls.message = Some(reason.replace("Apply", "Restore"));
							return PresetAction::Stay;
						}
						let path = &self.backups[self.backup_focus].path;
						match check_backup_folder(app, owner, path)
							.and_then(|()| owner.restore_library_backup(path, review))
						{
							Ok(count) => {
								self.refresh(app, owner, &format!("Restored {count} layouts"))
							}
							Err(error) => {
								self.page = Page::BackupReview(Err(error), RestorePage::Summary(0))
							}
						}
					}
				}
			}
		}
		PresetAction::Stay
	}

	fn refresh(&mut self, app: &mut GameApp, owner: &PresetTransfer, message: &str) {
		match owner.library().layouts() {
			Ok(layouts) => {
				self.layouts = layouts;
				self.focus = self.focus.min(self.layouts.len().saturating_sub(1));
				app.player_controls.message = Some(message.into());
			}
			Err(error) => {
				app.player_controls.message = Some(format!("{message}. Reopen the list: {error}"))
			}
		}
		self.page = Page::List;
	}
}

fn export_library(app: &GameApp, owner: &PresetTransfer) -> Result<String, String> {
	let stamp = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map_err(|error| error.to_string())?
		.as_millis();
	let folder = owner
		.transfer_directory(app.transfer_directory.as_deref(), true)?
		.ok_or("Input preset folder is unavailable")?;
	let count = owner.export_library(&folder.join(format!("layouts-{stamp}{BACKUP_SUFFIX}")))?;
	Ok(format!("Exported {count} layouts"))
}

fn list_backups(app: &GameApp, owner: &PresetTransfer) -> Result<Vec<BackupFile>, String> {
	let Some(folder) = owner.transfer_directory(app.transfer_directory.as_deref(), false)? else {
		return Ok(Vec::new());
	};
	Ok(transfer_entries(&folder, BACKUP_SUFFIX)?
		.into_iter()
		.enumerate()
		.map(|(index, (name, path))| BackupFile {
			label: transfer_label("Backup", index + 1, &name, BACKUP_SUFFIX),
			path,
		})
		.collect())
}

fn check_backup_folder(app: &GameApp, owner: &PresetTransfer, path: &Path) -> Result<(), String> {
	let folder = owner
		.transfer_directory(app.transfer_directory.as_deref(), false)?
		.ok_or("Input backup folder is unavailable")?;
	if path.parent() != Some(folder.as_path()) {
		return Err("Input backup folder changed. Reopen the backup list.".into());
	}
	Ok(())
}

fn restore_rows(
	title: &str,
	review: &Result<LibraryBackupReview, String>,
	page: &RestorePage,
	disabled: Option<&str>,
) -> (String, Vec<String>, usize) {
	let Ok(review) = review else {
		return (
			title.into(),
			vec!["Keep Current".into(), review.as_ref().unwrap_err().clone()],
			0,
		);
	};
	match page {
		RestorePage::Summary(focus) => {
			let mut rows = vec![
				"Keep Current".into(),
				format!("Add {} / {} free", review.retained(), review.available),
			];
			rows.extend(review.rows.iter().map(|row| match row.label() {
				Some(label) if row.conflict => format!("Rename: {} to {label}", row.source_label),
				Some(label) => format!("Add: {label}"),
				None if row.conflict => format!("Conflict: {} (skip)", row.source_label),
				None => format!("Skip: {}", row.source_label),
			}));
			rows.push(if let Some(reason) = review.blocked() {
				reason.into()
			} else if let Some(reason) = disabled {
				reason.replace("Apply", "Restore")
			} else {
				"Restore Layouts".into()
			});
			(title.into(), rows, *focus)
		}
		RestorePage::Choices { row, focus } => {
			let row = &review.rows[*row];
			let rows = if row.conflict {
				vec![
					format!("Current: {} (keep)", row.source_label),
					"Rename incoming".into(),
					"Skip incoming".into(),
					"Back".into(),
				]
			} else {
				vec!["Add incoming".into(), "Skip incoming".into(), "Back".into()]
			};
			(format!("Incoming: {}", row.source_label), rows, *focus)
		}
		RestorePage::Name { edit, .. } => {
			let (title, mut rows, focus) = edit.rows();
			rows[0] = format!("Use {}", edit.label);
			(title, rows, focus)
		}
	}
}

enum RestoreAction {
	Stay,
	Back,
	Commit,
}

fn restore_key(
	review: Option<&mut LibraryBackupReview>,
	page: &mut RestorePage,
	key: Key,
	message: &mut Option<String>,
) -> RestoreAction {
	let Some(review) = review else {
		return if matches!(key, Key::Escape | Key::Enter) {
			RestoreAction::Back
		} else {
			RestoreAction::Stay
		};
	};
	match page {
		RestorePage::Summary(focus) => {
			let length = 3 + review.rows.len();
			match key {
				Key::Escape | Key::Enter if key == Key::Escape || *focus == 0 => {
					return RestoreAction::Back;
				}
				Key::ArrowUp | Key::ArrowDown => {
					*focus = advance_row(*focus, length, key);
					*message = None;
				}
				Key::Enter if *focus + 1 == length && review.blocked().is_none() => {
					return RestoreAction::Commit;
				}
				Key::Enter if (2..length - 1).contains(focus) => {
					let row = *focus - 2;
					*page = RestorePage::Choices {
						row,
						focus: usize::from(review.rows[row].conflict),
					};
				}
				_ => {}
			}
		}
		RestorePage::Choices { row, focus } => {
			let conflict = review.rows[*row].conflict;
			let count = if conflict { 4 } else { 3 };
			match key {
				Key::Escape | Key::Enter if key == Key::Escape || *focus + 1 == count => {
					*page = RestorePage::Summary(*row + 2);
					*message = None;
				}
				Key::ArrowUp | Key::ArrowDown => *focus = advance_row(*focus, count, key),
				Key::Enter if *focus + 2 == count => {
					review.skip(*row);
					*page = RestorePage::Summary(*row + 2);
					*message = None;
				}
				Key::Enter if conflict && *focus == 1 => {
					let label = review.rows[*row]
						.label()
						.map_or_else(|| review.suggestion(*row), str::to_owned);
					*page = RestorePage::Name {
						row: *row,
						edit: NameEditor::new(label, false),
					};
				}
				Key::Enter if !conflict => {
					let source = review.rows[*row].source_label.clone();
					match review.use_label(*row, &source) {
						Ok(()) => {
							*page = RestorePage::Summary(*row + 2);
							*message = None;
						}
						Err(error) => *message = Some(error),
					}
				}
				_ => {}
			}
		}
		RestorePage::Name { row, edit } => match edit.key(key) {
			NameAction::Stay => {}
			NameAction::Cancel => {
				*page = RestorePage::Choices {
					row: *row,
					focus: 1,
				};
				*message = None;
			}
			NameAction::Save => match review.use_label(*row, &edit.label) {
				Ok(()) => {
					*page = RestorePage::Summary(*row + 2);
					*message = None;
				}
				Err(error) => *message = Some(error),
			},
		},
	}
	RestoreAction::Stay
}

enum NameAction {
	Stay,
	Save,
	Cancel,
}

struct NameEditor {
	label: String,
	focus: usize,
	character: usize,
	rename: bool,
}

impl NameEditor {
	fn new(label: String, rename: bool) -> Self {
		Self {
			label,
			focus: 0,
			character: 0,
			rename,
		}
	}

	fn rows(&self) -> (String, Vec<String>, usize) {
		let character = char::from(LABEL_CHARACTERS[self.character]);
		let add = if character == ' ' {
			"Add SPACE".into()
		} else {
			format!("Add {character}")
		};
		(
			format!("Name: {}", self.label),
			vec![
				format!("Save {}", self.label),
				add,
				"Erase last character".into(),
				"Cancel".into(),
			],
			self.focus,
		)
	}

	fn key(&mut self, key: Key) -> NameAction {
		match key {
			Key::Escape => return NameAction::Cancel,
			Key::ArrowUp | Key::ArrowDown => self.focus = advance_row(self.focus, 4, key),
			Key::ArrowLeft if self.focus == 1 => {
				self.character =
					(self.character + LABEL_CHARACTERS.len() - 1) % LABEL_CHARACTERS.len()
			}
			Key::ArrowRight if self.focus == 1 => {
				self.character = (self.character + 1) % LABEL_CHARACTERS.len()
			}
			Key::Enter => match self.focus {
				0 => return NameAction::Save,
				1 if self.label.len() < MAX_LABEL_BYTES => self
					.label
					.push(char::from(LABEL_CHARACTERS[self.character])),
				2 => {
					self.label.pop();
				}
				3 => return NameAction::Cancel,
				_ => {}
			},
			_ => {}
		}
		NameAction::Stay
	}
}
