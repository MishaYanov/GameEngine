//! Authored ordered routes and their bounded run state.

use std::{collections::HashSet, sync::Arc};

use engine_core::{project::ProjectData, scene_catalog::SceneId};
use serde::{Deserialize, Serialize};

use crate::settings::{InteractionSettings, SceneInteractions};

const MAX_CHAPTERS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChapterDefinition {
	pub id: String,
	pub label: String,
	pub course_scene: SceneId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CampaignDefinition {
	pub chapters: Vec<ChapterDefinition>,
	pub finish_scene: SceneId,
}

impl CampaignDefinition {
	pub(crate) fn validate(
		&self,
		project: &ProjectData,
		settings: &InteractionSettings,
	) -> Result<(), String> {
		if !(2..=MAX_CHAPTERS).contains(&self.chapters.len()) {
			return Err("A Courtyard campaign needs two to eight chapters".into());
		}
		if project.startup_scene_id != self.chapters[0].course_scene {
			return Err("The first chapter must be the authored startup scene".into());
		}
		let mut ids = HashSet::new();
		let mut course_scenes = HashSet::new();
		for (index, chapter) in self.chapters.iter().enumerate() {
			if !valid_chapter_id(&chapter.id) || !ids.insert(&chapter.id) {
				return Err(format!("Invalid or duplicate chapter ID {:?}", chapter.id));
			}
			if chapter.label.trim().is_empty()
				|| chapter.label.len() > 48
				|| !chapter
				.label
				.bytes()
				.all(|byte| (b' '..=b'~').contains(&byte))
			{
				return Err(format!("Invalid chapter label for {:?}", chapter.id));
			}
			if !course_scenes.insert(chapter.course_scene.as_str()) {
				return Err(format!(
					"Duplicate course scene {:?}",
					chapter.course_scene.as_str()
				));
			}
			let Some(SceneInteractions::Course {
				         destination_scene, ..
			         }) = settings.scenes.get(chapter.course_scene.as_str())
			else {
				return Err(format!(
					"Chapter {:?} needs a configured Course",
					chapter.id
				));
			};
			let expected = self
				.chapters
				.get(index + 1)
				.map_or(&self.finish_scene, |next| &next.course_scene);
			if destination_scene != expected {
				return Err(format!(
					"Chapter {:?} must lead to scene {:?}",
					chapter.id,
					expected.as_str()
				));
			}
		}
		if !matches!(
			settings.scenes.get(self.finish_scene.as_str()),
			Some(SceneInteractions::Finish { .. })
		) {
			return Err("The campaign finish needs configured Finish interactions".into());
		}
		for (id, scene) in &settings.scenes {
			match scene {
				SceneInteractions::Course { .. } if !course_scenes.contains(id.as_str()) => {
					return Err(format!(
						"Course scene {id:?} is missing from the chapter order"
					));
				}
				SceneInteractions::Finish { .. } if id != self.finish_scene.as_str() => {
					return Err(format!("Finish scene {id:?} is outside the chapter order"));
				}
				_ => {}
			}
		}
		Ok(())
	}

	/// Returns the sole authored successor of a chapter.
	pub fn successor(&self, scene: &SceneId) -> Option<&SceneId> {
		let index = self.index(scene)?;
		Some(
			self.chapters
				.get(index + 1)
				.map_or(&self.finish_scene, |chapter| &chapter.course_scene),
		)
	}

	pub(crate) fn index(&self, scene: &SceneId) -> Option<usize> {
		self.chapters
			.iter()
			.position(|chapter| &chapter.course_scene == scene)
	}
}

/// Durable gameplay facts scoped to one course.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CourseFacts {
	pub has_key: bool,
	pub gate_open: bool,
	pub exited: bool,
}

/// Bounded course progress, with an immutable validated chapter order.
#[derive(Clone, PartialEq, Eq)]
pub struct ChapterRun {
	pub(crate) route: Arc<CampaignDefinition>,
	pub(crate) gate_index: Option<usize>,
	facts: [CourseFacts; MAX_CHAPTERS],
}

impl ChapterRun {
	pub(crate) fn new(route: Arc<CampaignDefinition>, gate_scene: Option<&SceneId>) -> Self {
		let gate_index = gate_scene.and_then(|scene| route.index(scene));
		Self {
			route,
			gate_index,
			facts: [CourseFacts::default(); MAX_CHAPTERS],
		}
	}

	pub(crate) fn facts(&self, index: usize) -> CourseFacts {
		self.facts[index]
	}

	pub(crate) fn predecessors_exited(&self, index: usize) -> bool {
		index <= self.route.chapters.len() && self.facts[..index].iter().all(|facts| facts.exited)
	}

	pub(crate) fn all_exited(&self) -> bool {
		self.predecessors_exited(self.route.chapters.len())
	}

	pub(crate) fn assign(&mut self, index: usize, facts: CourseFacts) -> Result<(), String> {
		if index >= self.route.chapters.len() || !self.predecessors_exited(index) {
			return Err("The course requires all earlier chapter exits".into());
		}
		self.facts[index] = facts;
		Ok(())
	}

	pub(crate) fn validate(&self, score: i32, completed: bool) -> Result<(), String> {
		let count = self.route.chapters.len();
		let mut collected = 0;
		for (index, facts) in self.facts[..count].iter().enumerate() {
			let needs_gate = self.gate_index == Some(index);
			collected += usize::from(facts.has_key);
			if (*facts != CourseFacts::default() && !self.predecessors_exited(index))
				|| (facts.gate_open && (!needs_gate || !facts.has_key))
				|| (facts.exited && (!facts.has_key || (needs_gate && !facts.gate_open)))
			{
				return Err(format!(
					"Invalid chapter facts for {:?}",
					self.route.chapters[index].id
				));
			}
		}
		if self.facts[count..]
			.iter()
			.any(|facts| *facts != CourseFacts::default())
			|| (completed && !self.all_exited())
		{
			return Err("Invalid campaign score or completion facts".into());
		}
		if score < collected as i32 || score > i32::MAX - (count - collected) as i32 {
			return Err(
				"Chapter score must account for collected keys and allow remaining keys".into(),
			);
		}
		Ok(())
	}
}

pub(crate) fn valid_chapter_id(id: &str) -> bool {
	(1..=32).contains(&id.len())
		&& id.bytes().all(|byte| {
		byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
	})
}
