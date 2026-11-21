//! Chapter identity and the durable facts in a version 2 checkpoint.

use serde::{Deserialize, Deserializer, Serialize};

use super::{CampaignProgress, SavedFacts};
use crate::chapters::{ChapterRun, CourseFacts, valid_chapter_id};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SavedChapter {
	pub(super) id: String,
	completed_chapters: Vec<String>,
}

impl SavedChapter {
	pub(super) fn capture(run: &ChapterRun, index: usize) -> Result<Self, String> {
		if !run.predecessors_exited(index) {
			return Err("Checkpoint requires every preceding chapter to be cleared".into());
		}
		Ok(Self {
			id: run.route.chapters[index].id.clone(),
			completed_chapters: run.route.chapters[..index]
				.iter()
				.map(|chapter| chapter.id.clone())
				.collect(),
		})
	}

	pub(super) fn validate_shape(&self, saved: &SavedFacts) -> Result<(), String> {
		let prefix = &self.completed_chapters;
		if !valid_chapter_id(&self.id)
			|| prefix.len() > 7
			|| prefix.iter().enumerate().any(|(index, id)| {
			!valid_chapter_id(id) || id == &self.id || prefix[..index].contains(id)
		}) {
			return Err("Invalid saved chapter prefix".into());
		}
		if saved.score < prefix.len() as i32 + i32::from(saved.has_key)
			|| (saved.route_gate_open && !saved.has_key)
		{
			return Err("Invalid saved chapter score or key and gate flags".into());
		}
		Ok(())
	}

	pub(super) fn restore(
		&self,
		initial: &ChapterRun,
		index: usize,
		saved: &SavedFacts,
	) -> Result<CampaignProgress, String> {
		let mut run = initial.clone();
		if !self
			.completed_chapters
			.iter()
			.map(String::as_str)
			.eq(run.route.chapters[..index]
				.iter()
				.map(|chapter| chapter.id.as_str()))
		{
			return Err("Saved chapters do not match the preceding authored chapters".into());
		}
		for earlier in 0..index {
			run.assign(
				earlier,
				CourseFacts {
					has_key: true,
					gate_open: run.gate_index == Some(earlier),
					exited: true,
				},
			)?;
		}
		run.assign(
			index,
			CourseFacts {
				has_key: saved.has_key,
				gate_open: saved.route_gate_open,
				exited: false,
			},
		)?;
		Ok(CampaignProgress {
			score: saved.score,
			chapters: Some(run),
			..CampaignProgress::default()
		})
	}
}

// Absence preserves the V1 record identity. An explicit null is not a chapter.
pub(super) fn chapter_if_present<'de, D>(deserializer: D) -> Result<Option<SavedChapter>, D::Error>
where
	D: Deserializer<'de>,
{
	SavedChapter::deserialize(deserializer).map(Some)
}
