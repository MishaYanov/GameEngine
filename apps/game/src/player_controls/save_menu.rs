use super::{GameApp, Key, advance_row, continue_label};
use courtyard_gameplay::checkpoint::{
	ContinueAvailability, RestoreSource, SaveSlot, SlotGeneration, SlotInspection,
};

#[derive(Clone, Copy)]
pub(super) enum Intent {
	Save,
	Continue,
}

enum Page {
	Slots,
	Actions {
		focus: usize,
	},
	Confirm {
		replace: bool,
		generation: SlotGeneration,
		run: (String, u64),
	},
}

pub(super) struct SaveMenu {
	intent: Intent,
	selected: usize,
	slots: [SlotInspection; 3],
	page: Page,
}

impl SaveMenu {
	pub(super) fn new(app: &GameApp, intent: Intent) -> Self {
		Self {
			intent,
			selected: 0,
			slots: app.checkpoint_slots(),
			page: Page::Slots,
		}
	}

	pub(super) fn rows(&self) -> (String, Vec<String>, usize) {
		let selected = SaveSlot::ALL[self.selected];
		let slot = &self.slots[self.selected];
		match &self.page {
			Page::Slots => (
				"Choose save slot".into(),
				SaveSlot::ALL
					.iter()
					.zip(&self.slots)
					.map(|(slot, status)| {
						let label =
							format!("{} ({})", slot.label(), continue_label(status.primary));
						if status.recovery_checkpoint_id.is_some() {
							format!("{label}: prior save")
						} else if let (Some(chapter), Some(checkpoint)) =
							(&status.chapter_label, &status.checkpoint_id)
						{
							format!("{label}: {chapter} / {checkpoint}")
						} else if let Some(checkpoint) = &status.checkpoint_id {
							format!("{label}: {checkpoint}")
						} else {
							label
						}
					})
					.collect(),
				self.selected,
			),
			Page::Actions { focus } => {
				let rows = match self.intent {
					Intent::Save => vec![
						(if slot.generation.is_some_and(SlotGeneration::can_save) {
							"Save checkpoint"
						} else {
							"Save unavailable"
						})
						.into(),
						"Back".into(),
					],
					Intent::Continue => {
						let mut rows = vec![format!("Continue ({})", continue_label(slot.primary))];
						if let Some(checkpoint) = &slot.recovery_checkpoint_id {
							let saved = slot.recovery_chapter_label.as_ref().map_or_else(
								|| checkpoint.clone(),
								|chapter| format!("{chapter} / {checkpoint}"),
							);
							rows.push(format!("Recover prior ({saved})"));
						}
						rows.push("Back".into());
						rows
					}
				};
				(selected.label().into(), rows, *focus)
			}
			Page::Confirm { replace, .. } => (
				format!("Replace {}?", selected.label()),
				vec!["Cancel".into(), "Replace save".into()],
				usize::from(*replace),
			),
		}
	}

	/// Returns false when Escape leaves the slot picker.
	pub(super) fn key(&mut self, app: &mut GameApp, key: Key) -> bool {
		let slot = SaveSlot::ALL[self.selected];
		let snapshot = &self.slots[self.selected];
		match &mut self.page {
			Page::Slots => match key {
				Key::Escape => return false,
				Key::ArrowUp | Key::ArrowDown => {
					self.selected = advance_row(self.selected, 3, key);
				}
				Key::Enter => {
					self.slots[self.selected] = app.checkpoint_slot(slot);
					self.page = Page::Actions { focus: 0 };
				}
				_ => {}
			},
			Page::Actions { focus } => {
				let has_recovery = matches!(self.intent, Intent::Continue)
					&& snapshot.recovery_checkpoint_id.is_some();
				let back = if has_recovery { 2 } else { 1 };
				match key {
					Key::Escape => self.page = Page::Slots,
					Key::ArrowUp | Key::ArrowDown => *focus = advance_row(*focus, back + 1, key),
					Key::Enter if *focus == back => self.page = Page::Slots,
					Key::Enter => {
						let Some(generation) = snapshot.generation else {
							app.player_controls.message = Some("Save storage unavailable".into());
							return true;
						};
						if matches!(self.intent, Intent::Save) && !generation.can_save() {
							app.player_controls.message =
								Some("Save cannot replace the unreadable prior record".into());
							return true;
						}
						match self.intent {
							Intent::Save if snapshot.primary == ContinueAvailability::Empty => {
								let result = app.save_checkpoint_slot(slot, &generation);
								self.update(app, result);
							}
							Intent::Save => {
								if let Some(status) = app.editor.play_status() {
									self.page = Page::Confirm {
										replace: false,
										generation,
										run: (status.run_id, status.scene_generation),
									};
								}
							}
							Intent::Continue => {
								let source = if *focus == 0 {
									RestoreSource::Current
								} else {
									RestoreSource::Previous
								};
								let result = if app.play_trace.is_some() {
									Err("Continue is unavailable during playtrace recording".into())
								} else if *focus == 0 && !snapshot.primary.can_continue() {
									Err(continue_label(snapshot.primary).into())
								} else {
									app.continue_checkpoint_slot(slot, &generation, source)
								};
								self.update(app, result);
							}
						}
					}
					_ => {}
				}
			}
			Page::Confirm {
				replace,
				generation,
				run,
			} => match key {
				Key::ArrowDown | Key::ArrowRight => *replace = true,
				Key::ArrowUp | Key::ArrowLeft => *replace = false,
				Key::Escape => self.page = Page::Actions { focus: 0 },
				Key::Enter => {
					if *replace {
						let result = if app.editor.play_status().is_some_and(|status| {
							status.run_id == run.0 && status.scene_generation == run.1
						}) {
							app.save_checkpoint_slot(slot, generation)
						} else {
							Err("Play changed. Reopen the slot to save.".into())
						};
						self.update(app, result);
					}
					self.page = Page::Actions { focus: 0 };
				}
				_ => {}
			},
		}
		true
	}

	fn update(&mut self, app: &mut GameApp, result: Result<String, String>) {
		app.player_controls.message = Some(result.unwrap_or_else(|error| error));
		self.slots[self.selected] = app.checkpoint_slot(SaveSlot::ALL[self.selected]);
		self.page = Page::Actions { focus: 0 };
	}
}
