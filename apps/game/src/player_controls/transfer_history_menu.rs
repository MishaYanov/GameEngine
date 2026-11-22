//! Review or export the selected slot's checkpoint transfer history.

use super::{GameApp, Key, advance_row};
use crate::checkpoint_transfer_history::{
	ClearHistoryScope, Direction, HistoryEntry, HistoryRetention, HistoryView,
	ReviewedHistoryClear, ReviewedHistoryRetention,
};
use courtyard_gameplay::checkpoint::SaveSlot;
use std::time::{SystemTime, UNIX_EPOCH};

const RETENTION_LEVELS: [HistoryRetention; 3] = [
	HistoryRetention::Entries16,
	HistoryRetention::Entries32,
	HistoryRetention::Entries64,
];

#[derive(Clone, Copy)]
enum HistoryFilter {
	All,
	Exports,
	Imports,
}

impl HistoryFilter {
	fn label(self) -> &'static str {
		match self {
			Self::All => "All",
			Self::Exports => "Exports",
			Self::Imports => "Imports",
		}
	}

	fn next(self, forward: bool) -> Self {
		match (self, forward) {
			(Self::All, true) | (Self::Imports, false) => Self::Exports,
			(Self::Exports, true) | (Self::All, false) => Self::Imports,
			_ => Self::All,
		}
	}

	fn matches(self, direction: Direction) -> bool {
		matches!(
			(self, direction),
			(Self::All, _)
				| (Self::Exports, Direction::Export)
				| (Self::Imports, Direction::Import)
		)
	}
}

struct Row {
	entry: HistoryEntry,
	label: String,
}

enum Page {
	Entries,
	Retention(usize),
	ClearScope(usize),
	Review(Confirmation, usize),
}

enum Confirmation {
	Audit,
	Retention {
		choice: usize,
		review: ReviewedHistoryRetention,
	},
	Clear {
		choice: usize,
		review: ReviewedHistoryClear,
	},
}

pub(super) struct TransferHistoryMenu {
	slot: SaveSlot,
	rows: Vec<Row>,
	filter: HistoryFilter,
	retention: HistoryRetention,
	focus: usize,
	page: Page,
}

impl TransferHistoryMenu {
	pub(super) fn new(app: &GameApp, slot: SaveSlot) -> Result<Self, String> {
		let HistoryView { retention, entries } =
			app.with_checkpoint_history(|history| history.view(Some(slot)))?;
		Ok(Self {
			slot,
			rows: history_rows(entries),
			filter: HistoryFilter::All,
			retention,
			focus: 0,
			page: Page::Entries,
		})
	}

	pub(super) fn rows(&self) -> (String, Vec<String>, usize) {
		match &self.page {
			Page::Entries => self.entry_rows(),
			Page::Retention(focus) => (
				format!("Retention: {}", self.retention.limit()),
				vec![
					"Keep 16 entries".into(),
					"Keep 32 entries".into(),
					"Keep 64 entries".into(),
					"Back".into(),
				],
				*focus,
			),
			Page::ClearScope(focus) => (
				"Review Clear History".into(),
				vec![
					"Keep History".into(),
					self.slot.label().into(),
					"Current project".into(),
				],
				*focus,
			),
			Page::Review(confirmation, focus) => {
				let (title, rows) = match confirmation {
					Confirmation::Audit => (
						format!("{} audit preview", self.slot.label()),
						vec![
							"Cancel".into(),
							format!("Export {} history rows", self.matching().count()),
						],
					),
					Confirmation::Retention { choice, review } => (
						format!("Keep {} entries?", RETENTION_LEVELS[*choice].limit()),
						vec![
							"Keep History".into(),
							format!("Remove {} oldest entries", review.rows_to_remove),
						],
					),
					Confirmation::Clear { review, .. } => (
						format!(
							"{}: {} entries",
							match review.scope() {
								ClearHistoryScope::Slot(slot) => slot.label(),
								ClearHistoryScope::CurrentProject => "Current project",
							},
							review.rows_to_remove()
						),
						vec!["Keep History".into(), "Clear Reviewed Entries".into()],
					),
				};
				(title, rows, *focus)
			}
		}
	}

	fn entry_rows(&self) -> (String, Vec<String>, usize) {
		let title = format!("{} transfer history", self.slot.label());
		let mut rows = vec![format!("Filter: {}", self.filter.label())];
		rows.extend(self.matching().map(|row| row.label.clone()));
		if rows.len() == 1 {
			rows.push("No matching transfers".into());
		}
		rows.extend([
			"Export history audit".into(),
			format!("Retention: {}", self.retention.limit()),
			"Review Clear History".into(),
			"Back".into(),
		]);
		(title, rows, self.focus)
	}

	pub(super) fn key(&mut self, app: &mut GameApp, key: Key) -> bool {
		match &mut self.page {
			Page::Entries => return self.entry_key(app, key),
			Page::Retention(focus) => match key {
				Key::Escape => self.page = Page::Entries,
				Key::ArrowUp | Key::ArrowDown => *focus = advance_row(*focus, 4, key),
				Key::Enter if *focus == 3 => self.page = Page::Entries,
				Key::Enter if RETENTION_LEVELS[*focus] != self.retention => {
					let choice = *focus;
					self.choose_retention(app, choice);
				}
				_ => {}
			},
			Page::ClearScope(focus) => match key {
				Key::Escape | Key::Enter if key == Key::Escape || *focus == 0 => {
					self.page = Page::Entries;
				}
				Key::ArrowUp | Key::ArrowDown => *focus = advance_row(*focus, 3, key),
				Key::Enter => {
					let choice = *focus;
					self.choose_clear_scope(app, choice);
				}
				_ => {}
			},
			Page::Review(_, focus) => {
				match key {
					Key::ArrowDown | Key::ArrowUp | Key::ArrowLeft | Key::ArrowRight => {
						*focus = 1 - *focus;
						return true;
					}
					Key::Enter | Key::Escape => {}
					_ => return true,
				}
				let accept = key == Key::Enter && *focus == 1;
				if let Page::Review(review, _) = std::mem::replace(&mut self.page, Page::Entries) {
					self.finish_review(app, review, accept);
				}
			}
		}
		true
	}

	fn choose_retention(&mut self, app: &mut GameApp, choice: usize) {
		let result = app
			.with_checkpoint_history(|history| history.review_retention(RETENTION_LEVELS[choice]));
		app.player_controls.message = None;
		match result {
			Ok(review) if review.rows_to_remove != 0 => {
				self.page = Page::Review(Confirmation::Retention { choice, review }, 0);
			}
			Ok(review) => self.finish_review(app, Confirmation::Retention { choice, review }, true),
			Err(error) => app.player_controls.message = Some(error),
		}
	}

	fn choose_clear_scope(&mut self, app: &mut GameApp, choice: usize) {
		let scope = if choice == 1 {
			ClearHistoryScope::Slot(self.slot)
		} else {
			ClearHistoryScope::CurrentProject
		};
		let result = app.with_checkpoint_history(|history| history.review_clear(scope));
		app.player_controls.message = None;
		match result {
			Ok(review) if review.rows_to_remove() == 0 => {
				app.player_controls.message = Some("No history entries for this scope".into());
			}
			Ok(review) => self.page = Page::Review(Confirmation::Clear { choice, review }, 0),
			Err(error) => app.player_controls.message = Some(error),
		}
	}

	fn finish_review(&mut self, app: &mut GameApp, review: Confirmation, accept: bool) {
		match review {
			Confirmation::Audit => {
				if accept {
					let result = app.export_checkpoint_history(
						self.slot,
						self.matching().map(|row| &row.entry),
					);
					app.player_controls.message = Some(result.unwrap_or_else(|error| error));
				}
			}
			Confirmation::Retention { choice, review } => {
				self.page = Page::Retention(choice);
				if accept {
					let result =
						app.with_checkpoint_history(|history| history.set_retention(&review));
					let limit = RETENTION_LEVELS[choice].limit();
					match result {
						Ok(removed) => {
							self.reload(app, format!("Keeping {limit}; removed {removed}"))
						}
						Err(error) => app.player_controls.message = Some(error),
					}
				}
			}
			Confirmation::Clear { choice, review } => {
				self.page = Page::ClearScope(choice);
				if accept {
					let result =
						app.with_checkpoint_history(|history| history.clear_reviewed(&review));
					match result {
						Ok(removed) => {
							self.reload(app, format!("Cleared {removed} transfer entries"))
						}
						Err(error) => app.player_controls.message = Some(error),
					}
				}
			}
		}
	}

	fn reload(&mut self, app: &mut GameApp, message: String) {
		match app.with_checkpoint_history(|history| history.view(Some(self.slot))) {
			Ok(HistoryView { retention, entries }) => {
				self.retention = retention;
				self.rows = history_rows(entries);
				self.focus = 0;
				self.page = Page::Entries;
				app.player_controls.message = Some(message);
			}
			Err(error) => {
				app.player_controls.message = Some(format!("{message}. Reopen history: {error}"));
			}
		}
	}

	fn entry_key(&mut self, app: &mut GameApp, key: Key) -> bool {
		let export = 1 + self.matching().count().max(1);
		let retention = export + 1;
		let clear = export + 2;
		let back = export + 3;
		match key {
			Key::Escape => return false,
			Key::Enter if self.focus == back => return false,
			Key::ArrowUp | Key::ArrowDown => self.focus = advance_row(self.focus, back + 1, key),
			Key::ArrowLeft | Key::ArrowRight | Key::Enter
			if key != Key::Enter || self.focus == 0 =>
				{
					self.filter = self.filter.next(key != Key::ArrowLeft);
					self.focus = 0;
					app.player_controls.message = None;
				}
			Key::Enter if self.focus == export => {
				self.page = Page::Review(Confirmation::Audit, 0);
				app.player_controls.message = None;
			}
			Key::Enter if self.focus == retention => {
				let focus = RETENTION_LEVELS
					.iter()
					.position(|&level| level == self.retention)
					.unwrap_or(3);
				self.page = Page::Retention(focus);
				app.player_controls.message = None;
			}
			Key::Enter if self.focus == clear => {
				self.page = Page::ClearScope(0);
				app.player_controls.message = None;
			}
			_ => {}
		}
		true
	}

	fn matching(&self) -> impl Iterator<Item=&Row> {
		self.rows
			.iter()
			.filter(|row| self.filter.matches(row.entry.direction))
	}
}

fn history_rows(entries: Vec<HistoryEntry>) -> Vec<Row> {
	let now = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map_or(0, |time| time.as_secs());
	entries
		.into_iter()
		.map(|entry| {
			let action = match entry.direction {
				Direction::Export => "Exported",
				Direction::Import => "Imported",
			};
			let age = now.saturating_sub(entry.completed_unix_seconds);
			let age = if age < 60 {
				"now".into()
			} else if age < 3600 {
				format!("{}m ago", age / 60)
			} else if age < 86400 {
				format!("{}h ago", age / 3600)
			} else {
				format!("{}d ago", age / 86400)
			};
			let label = format!("{action} {} ({age})", entry.checkpoint_id);
			Row { entry, label }
		})
		.collect()
}
