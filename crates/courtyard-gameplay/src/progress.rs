use std::mem::size_of;

use engine_core::game_progress::GameProgress;
use engine_core::scene_catalog::SceneId;

use crate::chapters::{ChapterRun, CourseFacts};

#[derive(Clone, Default, PartialEq, Eq)]
pub struct CampaignProgress {
	pub score: i32,
	pub has_courtyard_key: bool,
	pub route_gate_open: bool,
	pub courtyard_exit_reached: bool,
	pub completed: bool,
	pub chapters: Option<ChapterRun>,
}

impl CampaignProgress {
	/// Returns the facts for the active course, or the original facts for a legacy run.
	pub fn scene_facts(&self, scene: &SceneId) -> CourseFacts {
		self.course_facts(
			self.chapters
				.as_ref()
				.and_then(|run| run.route.index(scene)),
		)
	}

	/// Identifies the course in a chaptered campaign.
	pub fn chapter_id(&self, scene: &SceneId) -> Option<&str> {
		let run = self.chapters.as_ref()?;
		Some(&run.route.chapters[run.route.index(scene)?].id)
	}

	pub(crate) fn course_facts(&self, index: Option<usize>) -> CourseFacts {
		match &self.chapters {
			Some(run) => index.map_or_else(CourseFacts::default, |index| run.facts(index)),
			None => CourseFacts {
				has_key: self.has_courtyard_key,
				gate_open: self.route_gate_open,
				exited: self.courtyard_exit_reached,
			},
		}
	}

	pub(crate) fn course_unlocked(&self, index: Option<usize>) -> bool {
		match &self.chapters {
			Some(run) => index.is_some_and(|index| run.predecessors_exited(index)),
			None => true,
		}
	}

	pub(crate) fn exits_reached(&self) -> bool {
		self.chapters
			.as_ref()
			.map_or(self.courtyard_exit_reached, ChapterRun::all_exited)
	}

	fn chapter_action(&mut self, action: &ProgressAction, index: usize) -> Result<(), String> {
		let run = self.chapters.as_mut().ok_or("The run has no chapters")?;
		if index >= run.route.chapters.len() || !run.predecessors_exited(index) {
			return Err("The course requires all earlier chapter exits".into());
		}
		let mut facts = run.facts(index);
		let mut score = self.score;
		match action {
			ProgressAction::CollectChapterKey { .. } => {
				if !facts.has_key {
					score = score.checked_add(1).ok_or("Campaign score overflow")?;
					facts.has_key = true;
				}
			}
			ProgressAction::OpenChapterGate { .. } => {
				if !facts.has_key || run.gate_index != Some(index) {
					return Err("Opening this route gate requires its chapter key".into());
				}
				facts.gate_open = true;
			}
			ProgressAction::ReachChapterExit { .. } => {
				if !facts.has_key || (run.gate_index == Some(index) && !facts.gate_open) {
					return Err("The chapter exit requires its key and an open route".into());
				}
				facts.exited = true;
			}
			_ => return Err("The action does not belong to a chapter".into()),
		}
		run.assign(index, facts)?;
		self.score = score;
		Ok(())
	}
}

pub enum ProgressAction {
	AwardScore { amount: i32 },
	CollectCourtyardKey,
	OpenRouteGate,
	ReachCourtyardExit,
	CollectChapterKey { index: usize },
	OpenChapterGate { index: usize },
	ReachChapterExit { index: usize },
	CompleteRun,
}

impl GameProgress for CampaignProgress {
	type Action = ProgressAction;
	fn validate(&self) -> Result<(), String> {
		if self.score < 0 {
			return Err("Campaign score must be nonnegative".into());
		}
		if let Some(run) = &self.chapters {
			if self.courtyard_exit_reached || self.route_gate_open || self.has_courtyard_key {
				return Err("Chaptered progress may only use chapter-scoped facts".into());
			}
			return run.validate(self.score, self.completed);
		}
		if self.courtyard_exit_reached && !self.has_courtyard_key {
			return Err("The courtyard exit cannot be reached without its key".into());
		}
		if self.route_gate_open && !self.has_courtyard_key {
			return Err("The route gate cannot be opened before collecting the key".into());
		}
		if self.completed && !self.courtyard_exit_reached {
			return Err("The run cannot be completed before reaching the courtyard exit".into());
		}
		Ok(())
	}
	fn retained_bytes(&self) -> usize {
		size_of::<Self>()
	}
	fn action_bytes(_: &ProgressAction) -> usize {
		size_of::<ProgressAction>()
	}
	fn apply(&mut self, action: &ProgressAction) -> Result<(), String> {
		match action {
			ProgressAction::CollectChapterKey { index }
			| ProgressAction::OpenChapterGate { index }
			| ProgressAction::ReachChapterExit { index } => {
				return self.chapter_action(action, *index);
			}
			ProgressAction::CollectCourtyardKey
			| ProgressAction::OpenRouteGate
			| ProgressAction::ReachCourtyardExit
			if self.chapters.is_some() =>
				{
					return Err("Chaptered runs require chapter-scoped actions".into());
				}
			_ => {}
		}
		match action {
			ProgressAction::AwardScore { amount } => {
				if *amount < 0 {
					return Err("Score awards must be nonnegative".into());
				}
				self.score = self
					.score
					.checked_add(*amount)
					.ok_or("Campaign score overflow")?;
			}
			ProgressAction::CollectCourtyardKey => {
				if !self.has_courtyard_key {
					let next_score = self.score.checked_add(1).ok_or("Campaign score overflow")?;

					self.score = next_score;
					self.has_courtyard_key = true;
				}
			}

			ProgressAction::OpenRouteGate => {
				if !self.has_courtyard_key {
					return Err("The route gate requires the courtyard key".into());
				}
				self.route_gate_open = true;
			}
			ProgressAction::ReachCourtyardExit => {
				if !self.has_courtyard_key {
					return Err("The courtyard exit requires the key".into());
				}

				self.courtyard_exit_reached = true;
			}
			ProgressAction::CompleteRun => {
				if !self.exits_reached() {
					return Err("Reach every course exit before completing the run".into());
				}
				self.completed = true;
			}
			ProgressAction::CollectChapterKey { .. }
			| ProgressAction::OpenChapterGate { .. }
			| ProgressAction::ReachChapterExit { .. } => unreachable!(),
		}
		Ok(())
	}
}
