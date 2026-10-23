use crate::Transform;
use crate::model_instance::ModelInstance;
use crate::scene::EntitySnapshot;
use crate::scene::{EntityId, Scene};

#[derive(Debug, Clone, Copy)]
pub struct SetTransform {
	pub entity: EntityId,
	pub transform: Transform,
}

#[derive(Debug, Clone, Copy)]
pub struct TransformChange {
	pub entity: EntityId,
	pub before: Transform,
	pub after: Transform,
}

const MAX_UNDO_ENTRIES: usize = 256;

#[derive(Clone)]
enum SceneChange {
	Transform(TransformChange),
	Spawn(EntitySnapshot),
	Delete(EntitySnapshot),
	Rename {
		entity: EntityId,
		before: String,
		after: String,
	},
}

impl std::fmt::Debug for SceneChange {
	fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Rename {
				entity,
				before,
				after,
			} => formatter
				.debug_tuple("Rename")
				.field(entity)
				.field(before)
				.field(after)
				.finish(),
			Self::Transform(change) => change.fmt(formatter),
			Self::Spawn(snapshot) => formatter
				.debug_tuple("Spawn")
				.field(&snapshot.id())
				.finish(),
			Self::Delete(snapshot) => formatter
				.debug_tuple("Delete")
				.field(&snapshot.id())
				.finish(),
		}
	}
}

#[derive(Debug, Clone, Copy)]
pub struct SceneEditEffect {
	pub entity: EntityId,
	pub present: bool,
}

#[derive(Debug, Default)]
pub struct CommandHistory {
	undo: Vec<SceneChange>,
	redo: Vec<SceneChange>,
}

impl CommandHistory {
	pub fn rename(
		&mut self,
		scene: &mut Scene,
		entity: EntityId,
		name: impl Into<String>,
	) -> Result<(), String> {
		let before = scene
			.name(entity)
			.ok_or("Cannot rename a missing entity")?
			.to_owned();
		let after = name.into();
		scene.rename(entity, after.clone())?;
		if before != after {
			self.push(SceneChange::Rename {
				entity,
				before,
				after,
			});
			self.redo.clear();
		}
		Ok(())
	}
	pub fn execute(
		&mut self,
		scene: &mut Scene,
		command: SetTransform,
		merge_with_previous: bool,
	) -> Result<Option<TransformChange>, String> {
		let change = command.apply(scene)?;
		if change.before == change.after {
			return Ok(None);
		}

		// The caller identifies frames belonging to the same movement gesture.
		let merge_target = if merge_with_previous && self.redo.is_empty() {
			self.undo.last_mut().and_then(|previous| match previous {
				SceneChange::Transform(previous) if previous.entity == change.entity => {
					Some(previous)
				}
				_ => None,
			})
		} else {
			None
		};

		if let Some(previous) = merge_target {
			previous.after = change.after;
		} else {
			self.push(SceneChange::Transform(change));
		}
		self.redo.clear();

		Ok(Some(change))
	}

	pub fn undo(&mut self, scene: &mut Scene) -> Result<Option<TransformChange>, String> {
		let change = match self.undo.last() {
			Some(SceneChange::Transform(change)) => *change,
			Some(_) => return Err("Use undo_scene for entity operations".to_string()),
			None => return Ok(None),
		};
		self.undo_scene(scene)?;
		Ok(Some(change))
	}

	pub fn redo(&mut self, scene: &mut Scene) -> Result<Option<TransformChange>, String> {
		let change = match self.redo.last() {
			Some(SceneChange::Transform(change)) => *change,
			Some(_) => return Err("Use redo_scene for entity operations".to_string()),
			None => return Ok(None),
		};
		self.redo_scene(scene)?;
		Ok(Some(change))
	}

	pub fn spawn(
		&mut self,
		scene: &mut Scene,
		name: impl Into<String>,
		path: impl Into<String>,
		instance: ModelInstance,
	) -> Result<EntityId, String> {
		let id = scene.spawn_persistent_model(name, path, instance)?;
		let snapshot = scene.snapshot(id)?;
		self.push(SceneChange::Spawn(snapshot));
		self.redo.clear();
		Ok(id)
	}

	pub fn duplicate(
		&mut self,
		scene: &mut Scene,
		source: EntityId,
		offset: [f32; 3],
	) -> Result<EntityId, String> {
		let id = scene.duplicate(source, offset)?;
		let snapshot = scene.snapshot(id)?;
		self.push(SceneChange::Spawn(snapshot));
		self.redo.clear();
		Ok(id)
	}

	pub fn delete(&mut self, scene: &mut Scene, id: EntityId) -> Result<(), String> {
		let snapshot = scene.snapshot(id)?;
		scene
			.remove(id)
			.ok_or("Entity disappeared before deletion")?;
		self.push(SceneChange::Delete(snapshot));
		self.redo.clear();
		Ok(())
	}

	pub fn undo_scene(&mut self, scene: &mut Scene) -> Result<Option<SceneEditEffect>, String> {
		let Some(change) = self.undo.last() else {
			return Ok(None);
		};
		let effect = Self::apply_change(scene, change, false)?;
		let change = self
			.undo
			.pop()
			.expect("record remains until successful restoration");
		self.redo.push(change);
		Ok(Some(effect))
	}

	pub fn redo_scene(&mut self, scene: &mut Scene) -> Result<Option<SceneEditEffect>, String> {
		let Some(change) = self.redo.last() else {
			return Ok(None);
		};
		let effect = Self::apply_change(scene, change, true)?;
		let change = self
			.redo
			.pop()
			.expect("record remains until successful restoration");
		self.undo.push(change);
		Ok(Some(effect))
	}

	fn push(&mut self, change: SceneChange) {
		self.undo.push(change);
		if self.undo.len() > MAX_UNDO_ENTRIES {
			self.undo.remove(0);
		}
	}

	fn apply_change(
		scene: &mut Scene,
		change: &SceneChange,
		forward: bool,
	) -> Result<SceneEditEffect, String> {
		match change {
			SceneChange::Rename {
				entity,
				before,
				after,
			} => {
				scene.rename(*entity, if forward { after } else { before })?;
				Ok(SceneEditEffect {
					entity: *entity,
					present: true,
				})
			}
			SceneChange::Transform(change) => {
				SetTransform {
					entity: change.entity,
					transform: if forward { change.after } else { change.before },
				}
					.apply(scene)?;
				Ok(SceneEditEffect {
					entity: change.entity,
					present: true,
				})
			}
			SceneChange::Spawn(snapshot) | SceneChange::Delete(snapshot) => {
				let restore = matches!(change, SceneChange::Spawn(_)) == forward;
				if restore {
					scene.restore(snapshot)?;
				} else {
					scene.remove(snapshot.id()).ok_or_else(|| {
						format!("Cannot remove missing entity {:?}", snapshot.id())
					})?;
				}
				Ok(SceneEditEffect {
					entity: snapshot.id(),
					present: restore,
				})
			}
		}
	}

	pub fn clear(&mut self) {
		self.undo.clear();
		self.redo.clear();
	}
}

impl SetTransform {
	pub fn apply(self, scene: &mut Scene) -> Result<TransformChange, String> {
		self.transform.validate()?;

		let instance = scene
			.get_mut(self.entity)
			.ok_or_else(|| format!("Entity {:?} was not found", self.entity))?;

		let before = *instance.transform();
		*instance.transform_mut() = self.transform;

		Ok(TransformChange {
			entity: self.entity,
			before,
			after: self.transform,
		})
	}
}

#[cfg(test)]
#[path = "tests_commands.rs"]
mod tests;
