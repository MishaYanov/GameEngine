//! Review existing-object transforms in a paused Play world before authoring them.
use super::*;
use crate::command::{
	PlayTransformChangeDto, PlayTransformReviewDto, TransformDto, TransformEditDto, TransformSpace,
};

pub(crate) const MAX_PLAY_TRANSFORM_CHANGES: usize = 256;

impl EditorSession {
	pub fn review_play_transforms(&self) -> Result<PlayTransformReviewDto, String> {
		let play = self
			.play
			.as_ref()
			.ok_or("Start Play before reviewing changes")?;
		let active = play.active();
		if !active.paused() {
			return Err("Pause Play before reviewing runtime transforms".into());
		}
		if active.scene_id() != &self.active_scene_id {
			return Err("The running scene differs from the current authored scene".into());
		}

		let mut changes = Vec::new();
		let mut truncated = false;
		let world = active.world();
		for before in &self.data.entities {
			let Some(index) = world.entity_index(&before.key) else {
				continue;
			};
			let after = &world.data().entities[index];
			if before.transform == after.transform
				|| before.parent != after.parent
				|| !active
					.runtime_handle(&before.key)
					.is_some_and(|handle| handle.generation == 0)
			{
				continue;
			}
			if changes.len() == MAX_PLAY_TRANSFORM_CHANGES {
				truncated = true;
				break;
			}
			let dto = |transform: &crate::scene_data::TransformData| TransformDto {
				position: transform.position,
				rotation: transform.rotation,
				scale: transform.scale,
			};
			changes.push(PlayTransformChangeDto {
				key: before.key.clone(),
				name: before.name.clone(),
				before: dto(&before.transform),
				after: dto(&after.transform),
			});
		}
		Ok(PlayTransformReviewDto {
			run_id: play.run_id().into(),
			scene_id: self.active_scene_id.clone(),
			revision: self.revision,
			scene_revision: self.scene_revision,
			changes,
			truncated,
		})
	}

	pub(super) fn apply_play_transforms(
		&mut self,
		run_id: &str,
		scene_revision: u64,
		keys: Vec<EntityKey>,
	) -> Result<(), String> {
		let review = self.review_play_transforms()?;
		if review.run_id != run_id || review.scene_revision != scene_revision {
			return Err("Play changed. Review the current transforms again".into());
		}
		let by_key: HashMap<_, _> = review
			.changes
			.into_iter()
			.map(|item| (item.key, item.after))
			.collect();
		let edits = keys
			.into_iter()
			.map(|key| {
				let transform = by_key.get(&key).ok_or_else(|| {
					format!(
						"Object {} is no longer in the transform review",
						key.as_str()
					)
				})?;
				Ok(TransformEditDto {
					key: key.as_str().into(),
					transform: transform.clone(),
				})
			})
			.collect::<Result<Vec<_>, String>>()?;
		let mut next = (*self.data).clone();
		let mut selection = self.selection.clone();
		operations::apply(
			&mut next,
			&mut selection,
			&self.snap,
			&self.bounds,
			Command::SetTransforms {
				edits,
				space: TransformSpace::Local,
				snap: false,
			},
		)?;

		// Keep the paused world available if authored validation or history admission fails.
		let paused = self.play.take();
		if let Err(error) = self.commit(next, selection, "Apply Play transforms") {
			self.play = paused;
			return Err(error);
		}
		Ok(())
	}
}
