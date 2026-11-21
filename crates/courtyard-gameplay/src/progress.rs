use std::mem::size_of;

use engine_core::game_progress::GameProgress;

#[derive(Clone, Default, PartialEq, Eq)]
pub struct CampaignProgress {
	pub score: i32,
	pub has_courtyard_key: bool,
	pub courtyard_exit_reached: bool,
	pub completed: bool,
}

pub enum ProgressAction {
	AwardScore { amount: i32 },
	CollectCourtyardKey,
	ReachCourtyardExit,
	CompleteRun,
}

impl GameProgress for CampaignProgress {
	type Action = ProgressAction;
	fn validate(&self) -> Result<(), String> {
		if self.score < 0 {
			return Err("Campaign score must be nonnegative".into());
		}
		if self.courtyard_exit_reached && !self.has_courtyard_key {
			return Err("The courtyard exit cannot be reached without its key".into());
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

			ProgressAction::ReachCourtyardExit => {
				if !self.has_courtyard_key {
					return Err("The courtyard exit requires the key".into());
				}

				self.courtyard_exit_reached = true;
			}
			ProgressAction::CompleteRun => {
				if !self.courtyard_exit_reached {
					return Err("Reach the courtyard exit before completing the run".into());
				}
				self.completed = true;
			}
		}
		Ok(())
	}
}
