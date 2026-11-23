//! Player Controls for Cinder Oath's portable Practice slot.

use super::advance_row;
use crate::{
	GameApp,
	cinder_checkpoint::{
		Attempt, CHECKPOINT_SUFFIX, PracticeStore, Preflight, Review, TransferFile,
		audit::{AUDIT_SUFFIX, AuditReview, Direction},
		publish_transfer,
	},
	local_player_files::{transfer_entries, transfer_label},
};
use runtime::Key;

const SLOT_LABEL: &str = "Practice slot";
const MAIN_ACTIONS: usize = 8;

pub(super) enum MenuAction {
	Stay,
	Back,
	Resume,
}

#[derive(Clone, Copy)]
enum PickKind {
	Checkpoint,
	Audit,
}

impl PickKind {
	fn suffix(self) -> &'static str {
		match self {
			Self::Checkpoint => CHECKPOINT_SUFFIX,
			Self::Audit => AUDIT_SUFFIX,
		}
	}

	fn label(self) -> &'static str {
		match self {
			Self::Checkpoint => "Checkpoint",
			Self::Audit => "Audit",
		}
	}
}

struct Picker {
	kind: PickKind,
	files: Vec<TransferFile>,
	focus: usize,
}

impl Picker {
	fn selected(&self) -> Option<&TransferFile> {
		self.focus
			.checked_sub(1)
			.and_then(|index| self.files.get(index))
	}
}

enum Screen {
	Main,
	Pick(Picker),
	Review {
		picker: Picker,
		review: Review,
		focus: usize,
	},
	Recheck {
		picker: Picker,
		focus: usize,
	},
	Audit {
		title: String,
		audit: AuditReview,
		focus: usize,
	},
	ExportAudit {
		frozen: Vec<u8>,
		focus: usize,
	},
}

pub(super) struct CheckpointMenu {
	store: PracticeStore,
	focus: usize,
	direction: Option<Direction>,
	screen: Screen,
}

impl CheckpointMenu {
	pub(super) fn new(app: &GameApp) -> Result<Self, String> {
		let profile = app
			.player_controls
			.preferences
			.as_ref()
			.ok_or("Local Player profile is unavailable")?;
		let manifest = app
			.project_path
			.as_deref()
			.ok_or("Installed project is unavailable")?;
		Ok(Self {
			store: PracticeStore::new(profile.profile_root(), manifest)?,
			focus: 0,
			direction: None,
			screen: Screen::Main,
		})
	}

	pub(super) fn rows(&self) -> (String, Vec<String>, usize) {
		match &self.screen {
			Screen::Main => (
				"Cinder Oath Checkpoints".into(),
				vec![
					"Save Arena Entry".into(),
					"Continue Practice Slot".into(),
					"Export Checkpoint".into(),
					"Import Checkpoint".into(),
					format!(
						"Audit filter: {}",
						self.direction.map_or("All", Direction::label)
					),
					"Export Audit".into(),
					"Review Audit File".into(),
					"Back".into(),
				],
				self.focus,
			),
			Screen::Pick(picker) => (
				format!("Choose {} File", picker.kind.label()),
				std::iter::once("Back".into())
					.chain(picker.files.iter().enumerate().map(|(index, file)| {
						transfer_label(
							picker.kind.label(),
							index + 1,
							&file.name,
							picker.kind.suffix(),
						)
					}))
					.collect(),
				picker.focus,
			),
			Screen::Review {
				picker,
				review,
				focus,
			} => (
				"Review Checkpoint Import".into(),
				vec![
					"Cancel".into(),
					if review.replaces {
						"Replace Practice Slot"
					} else {
						"Import to Practice Slot"
					}
					.into(),
					transfer_label("File", picker.focus, review.filename(), CHECKPOINT_SUFFIX),
					format!("Entry: {}", review.archive.id()),
					format!("File SHA-256: {}", digest_prefix(&review.source_sha256)),
					format!(
						"Slot SHA-256: {}",
						digest_prefix(&review.destination_sha256)
					),
				],
				*focus,
			),
			Screen::Recheck { focus, .. } => (
				"Checkpoint Import Paused".into(),
				vec!["Back".into(), "Recheck Selected File".into()],
				*focus,
			),
			Screen::Audit {
				title,
				audit,
				focus,
			} => (title.clone(), audit_rows(audit), *focus),
			Screen::ExportAudit { focus, .. } => (
				"Export Practice Slot Audit".into(),
				vec!["Cancel".into(), "Export Reviewed Audit".into()],
				*focus,
			),
		}
	}

	pub(super) fn key(&mut self, app: &mut GameApp, key: Key) -> MenuAction {
		let screen = std::mem::replace(&mut self.screen, Screen::Main);
		self.screen = match screen {
			Screen::Main => return self.main_key(app, key),
			Screen::Pick(mut picker) => {
				navigate(&mut picker.focus, picker.files.len() + 1, key);
				match key {
					Key::Escape => Screen::Main,
					Key::Enter => match picker.selected() {
						None => Screen::Main,
						Some(_) => self.open_selected(app, picker),
					},
					_ => Screen::Pick(picker),
				}
			}
			Screen::Review {
				picker,
				review,
				mut focus,
			} => {
				navigate(&mut focus, 6, key);
				match key {
					Key::Escape => Screen::Pick(picker),
					Key::Enter if focus == 0 => Screen::Pick(picker),
					Key::Enter if focus == 1 => self.confirm_import(app, picker, &review),
					_ => Screen::Review {
						picker,
						review,
						focus,
					},
				}
			}
			Screen::Recheck { picker, mut focus } => {
				navigate(&mut focus, 2, key);
				match key {
					Key::Escape => Screen::Pick(picker),
					Key::Enter if focus == 0 => Screen::Pick(picker),
					Key::Enter => self.open_selected(app, picker),
					_ => Screen::Recheck { picker, focus },
				}
			}
			Screen::Audit {
				title,
				audit,
				mut focus,
			} => {
				// There are six detail rows and either the receipt list or its empty state.
				navigate(&mut focus, 7 + audit.entries.len().max(1), key);
				if key == Key::Escape || key == Key::Enter && focus == 0 {
					Screen::Main
				} else {
					Screen::Audit {
						title,
						audit,
						focus,
					}
				}
			}
			Screen::ExportAudit { frozen, mut focus } => {
				navigate(&mut focus, 2, key);
				match key {
					Key::Escape => Screen::Main,
					Key::Enter if focus == 0 => Screen::Main,
					Key::Enter => {
						let result = self.publish(app, "practice", AUDIT_SUFFIX, &frozen);
						set_result(app, result, "Exported Practice slot audit");
						Screen::Main
					}
					_ => Screen::ExportAudit { frozen, focus },
				}
			}
		};
		MenuAction::Stay
	}

	fn main_key(&mut self, app: &mut GameApp, key: Key) -> MenuAction {
		navigate(&mut self.focus, MAIN_ACTIONS, key);
		if key == Key::Escape || key == Key::Enter && self.focus == MAIN_ACTIONS - 1 {
			return MenuAction::Back;
		}
		if self.focus == 4 && matches!(key, Key::ArrowLeft | Key::ArrowRight | Key::Enter) {
			let filters = [None, Some(Direction::Export), Some(Direction::Import)];
			let index = filters
				.iter()
				.position(|filter| *filter == self.direction)
				.unwrap_or(0);
			let step = if key == Key::ArrowLeft {
				filters.len() - 1
			} else {
				1
			};
			self.direction = filters[(index + step) % filters.len()];
			app.player_controls.message = None;
		}
		if key == Key::Enter {
			match self.focus {
				0 => {
					let result = self.save_entry(app);
					set_result(app, result, "Saved Arena Entry");
				}
				1 => {
					let result = self
						.required_checkpoint()
						.and_then(|checkpoint| app.resume_checkpoint_arena(&checkpoint));
					if result.is_ok() {
						app.player_controls.message = None;
						return MenuAction::Resume;
					}
					set_result(app, result, "");
				}
				2 => {
					let result = self.export_checkpoint(app);
					match result {
						Ok(message) | Err(message) => app.player_controls.message = Some(message),
					}
				}
				3 => self.screen = self.pick(app, PickKind::Checkpoint),
				5 => match self.store.audit(self.direction) {
					Ok(frozen) => {
						app.player_controls.message = None;
						self.screen = Screen::ExportAudit { frozen, focus: 0 };
					}
					Err(error) => app.player_controls.message = Some(error),
				},
				6 => self.screen = self.pick(app, PickKind::Audit),
				_ => {}
			}
		}
		MenuAction::Stay
	}

	fn pick(&self, app: &mut GameApp, kind: PickKind) -> Screen {
		let result = self
			.store
			.transfer_directory(app.transfer_directory.as_deref(), false)
			.and_then(|directory| match directory {
				Some(directory) => Ok(transfer_entries(&directory, kind.suffix())?
					.into_iter()
					.map(|(name, _)| TransferFile::new(&directory, name, kind.suffix()))
					.collect()),
				None => Ok(Vec::new()),
			});
		match result {
			Ok(files) => {
				app.player_controls.message = files
					.is_empty()
					.then(|| format!("No {} files in transfer folder", kind.label()));
				Screen::Pick(Picker {
					kind,
					files,
					focus: 0,
				})
			}
			Err(problem) => {
				app.player_controls.message = Some(problem);
				Screen::Main
			}
		}
	}

	fn open_selected(&self, app: &mut GameApp, picker: Picker) -> Screen {
		let Some(file) = picker.selected() else {
			return Screen::Pick(picker);
		};
		match picker.kind {
			PickKind::Checkpoint => match self.store.preflight(file, || {
				app.prepare_checkpoint_arena().map(|arena| arena.checkpoint)
			}) {
				Preflight::Ready(review) => {
					app.player_controls.message = Some("Arena entry matches installed game".into());
					Screen::Review {
						picker,
						review: *review,
						focus: 0,
					}
				}
				Preflight::Blocked(problem) => {
					app.player_controls.message = Some(problem.label().into());
					Screen::Recheck { picker, focus: 0 }
				}
			},
			PickKind::Audit => match file.path().and_then(|path| AuditReview::read(&path)) {
				Ok(audit) => {
					app.player_controls.message = None;
					let title = transfer_label("Audit", picker.focus, &file.name, AUDIT_SUFFIX);
					Screen::Audit {
						title,
						audit,
						focus: 0,
					}
				}
				Err(error) => {
					app.player_controls.message = Some(error);
					Screen::Pick(picker)
				}
			},
		}
	}

	fn confirm_import(&self, app: &mut GameApp, picker: Picker, review: &Review) -> Screen {
		if app.play_trace.is_some() {
			app.player_controls.message = Some("Import unavailable while recording".into());
			return Screen::Pick(picker);
		}
		match self.store.import(review, || {
			app.prepare_checkpoint_arena().map(|arena| arena.checkpoint)
		}) {
			Attempt::Committed => {
				app.player_controls.message = Some("Imported into Practice slot".into());
				Screen::Main
			}
			Attempt::NoSlotWrite(cause) => {
				app.player_controls.message = Some(cause.label().into());
				Screen::Recheck { picker, focus: 0 }
			}
			Attempt::InspectSlot(error) => {
				engine_logger::warn!("Cinder Oath practice slot publication failed: {error}");
				app.player_controls.message = Some("Import failed. Inspect Practice slot.".into());
				Screen::Main
			}
		}
	}

	fn save_entry(&self, app: &GameApp) -> Result<(), String> {
		if app.play_trace.is_some() {
			return Err("Save unavailable while recording".into());
		}
		if app.checkpoint_arena_scene().as_ref() != app.editor.play_scene_id()
			|| app.editor.play_scene_id().is_none()
		{
			return Err("Enter the arena before saving".into());
		}
		let arena = app
			.prepare_checkpoint_arena()
			.map_err(|problem| problem.label())?;
		self.store.save(arena.checkpoint)
	}

	fn required_checkpoint(&self) -> Result<crate::cinder_checkpoint::Checkpoint, String> {
		self.store
			.checkpoint()?
			.ok_or_else(|| "Practice slot is empty".into())
	}

	fn export_checkpoint(&self, app: &GameApp) -> Result<String, String> {
		let checkpoint = self.required_checkpoint()?;
		let bytes = serde_json::to_vec_pretty(&checkpoint).map_err(|error| error.to_string())?;
		self.publish(app, "arena-entry", CHECKPOINT_SUFFIX, &bytes)?;
		match self.store.record_export() {
			Ok(()) => Ok("Exported arena checkpoint".into()),
			Err(error) => {
				engine_logger::warn!(
					"Cinder Oath checkpoint exported without audit receipt: {error}"
				);
				Ok("Checkpoint exported. Audit unavailable.".into())
			}
		}
	}

	fn publish(
		&self,
		app: &GameApp,
		prefix: &str,
		suffix: &str,
		bytes: &[u8],
	) -> Result<(), String> {
		let directory = self
			.store
			.transfer_directory(app.transfer_directory.as_deref(), true)?
			.ok_or("Cannot open the checkpoint transfer folder")?;
		publish_transfer(&directory, prefix, suffix, bytes)
	}
}

fn audit_rows(audit: &AuditReview) -> Vec<String> {
	let scope = audit.scope.as_ref();
	let timestamp = |time: Option<u64>| time.map_or_else(|| "-".into(), |time| time.to_string());
	let mut rows = vec![
		"Close".into(),
		format!(
			"Project: {}",
			scope.map_or("Unspecified", |scope| scope.project_id.as_str())
		),
		format!(
			"Slot: {}",
			scope.map_or("Unspecified", |scope| if scope.slot.is_some() {
				SLOT_LABEL
			} else {
				"All"
			})
		),
		format!(
			"Direction: {}",
			scope.map_or("Unspecified", |scope| scope
				.direction
				.map_or("All", Direction::label))
		),
		format!(
			"Rows: {}  SHA-256: {}",
			audit.entries.len(),
			digest_prefix(&audit.source_sha256)
		),
		format!("First (Unix s): {}", timestamp(audit.first_completed)),
		format!("Last (Unix s): {}", timestamp(audit.last_completed)),
	];
	if audit.entries.is_empty() {
		rows.push("No transfer receipts".into());
	} else {
		rows.extend(audit.entries.iter().map(|row| {
			format!(
				"{} #{} {} @{}",
				row.direction.label(),
				row.slot,
				row.checkpoint_id,
				row.completed_unix_seconds,
			)
		}));
	}
	rows
}

fn navigate(focus: &mut usize, count: usize, key: Key) {
	if matches!(key, Key::ArrowUp | Key::ArrowDown) {
		*focus = advance_row(*focus, count, key);
	}
}

fn digest_prefix(digest: &[u8; 32]) -> String {
	digest[..6]
		.iter()
		.map(|byte| format!("{byte:02x}"))
		.collect()
}

fn set_result(app: &mut GameApp, result: Result<(), String>, success: &str) {
	app.player_controls.message = Some(match result {
		Ok(()) => success.into(),
		Err(error) => error,
	});
}
