//! Review and confirmation pages for transfer files in a selected save slot.
use super::{GameApp, Key, advance_row};
use crate::checkpoint_transfer::TransferFile;
use courtyard_gameplay::checkpoint::{ImportPreview, SaveSlot};

pub(super) enum TransferAction {
	Stay,
	Back,
	Imported(String),
}

struct Review {
	preview: ImportPreview,
	confirm: bool,
}

pub(super) struct TransferMenu {
	slot: SaveSlot,
	folder: String,
	project: String,
	files: Vec<TransferFile>,
	focus: usize,
	review: Option<Review>,
}

impl TransferMenu {
	pub(super) fn new(app: &GameApp, slot: SaveSlot) -> Result<Self, String> {
		Ok(Self {
			slot,
			folder: app.checkpoint_transfer_folder_label(),
			project: app
				.project_name
				.clone()
				.unwrap_or_else(|| "Current project".into()),
			files: app.checkpoint_transfer_files()?,
			focus: 0,
			review: None,
		})
	}

	pub(super) fn rows(&self) -> (String, Vec<String>, usize) {
		if let Some(review) = &self.review {
			let mut rows = vec![self.files[self.focus].label.clone(), self.project.clone()];
			if let Some(chapter) = &review.preview.chapter_label {
				rows.push(format!("Chapter: {chapter}"));
			}
			rows.push(format!("Checkpoint: {}", review.preview.checkpoint_id));
			let focus = rows.len() + usize::from(review.confirm);
			rows.push("Cancel".into());
			rows.push(if review.preview.replacement_required {
				"Replace and keep backup".into()
			} else {
				"Import checkpoint".into()
			});
			return (format!("Review {}", self.slot.label()), rows, focus);
		}
		let mut rows: Vec<_> = self.files.iter().map(|file| file.label.clone()).collect();
		let title = if rows.is_empty() {
			format!("No transfers in {}", self.folder)
		} else {
			format!("From {}", self.folder)
		};
		rows.push("Back".into());
		(title, rows, self.focus)
	}

	pub(super) fn key(&mut self, app: &mut GameApp, key: Key) -> TransferAction {
		if let Some(review) = self.review.as_mut() {
			match key {
				Key::ArrowRight | Key::ArrowDown => review.confirm = true,
				Key::ArrowLeft | Key::ArrowUp => review.confirm = false,
				Key::Escape | Key::Enter => {
					if key == Key::Enter && review.confirm {
						let result = app.import_checkpoint_transfer(
							self.slot,
							&self.files[self.focus],
							&review.preview,
						);
						self.review = None;
						match result {
							Ok(message) => return TransferAction::Imported(message),
							Err(error) => {
								app.player_controls.message = Some(error);
								if let Ok(files) = app.checkpoint_transfer_files() {
									self.files = files;
									self.focus = self.focus.min(self.files.len());
								}
							}
						}
					} else {
						self.review = None;
						app.player_controls.message = None;
					}
				}
				_ => {}
			}
			return TransferAction::Stay;
		}
		match key {
			Key::Escape => return TransferAction::Back,
			Key::ArrowUp | Key::ArrowDown => {
				self.focus = advance_row(self.focus, self.files.len() + 1, key);
			}
			Key::Enter => {
				let Some(file) = self.files.get(self.focus) else {
					return TransferAction::Back;
				};
				match app.preview_checkpoint_transfer(self.slot, file) {
					Ok(preview) => {
						self.review = Some(Review {
							preview,
							confirm: false,
						});
						app.player_controls.message = None;
					}
					Err(error) => app.player_controls.message = Some(error),
				}
			}
			_ => {}
		}
		TransferAction::Stay
	}
}
