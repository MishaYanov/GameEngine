use crate::scene::{EntityId, Scene};

#[derive(Debug, Default)]
pub struct EntitySelection {
	selected: Option<EntityId>,
}

impl EntitySelection {
	pub fn selected(&self) -> Option<EntityId> {
		self.selected
	}

	pub fn select(&mut self, scene: &Scene, entity: EntityId) -> Result<(), String> {
		if !scene.contains(entity) {
			return Err(format!("Cannot select missing entity {entity:?}"));
		}
		self.selected = Some(entity);
		Ok(())
	}

	pub fn clear(&mut self) {
		self.selected = None;
	}

	pub fn clear_if_missing(&mut self, scene: &Scene) {
		if self.selected.is_some_and(|entity| !scene.contains(entity)) {
			self.clear();
		}
	}

	pub fn is_selected(&self, entity: EntityId) -> bool {
		self.selected == Some(entity)
	}

	pub fn select_next(&mut self, scene: &Scene) {
		let first = scene.entity_ids().next();

		let Some(current) = self.selected else {
			self.selected = first;
			return;
		};

		let mut ids = scene.entity_ids();

		self.selected = if ids.position(|id| id == current).is_some() {
			ids.next().or(first)
		} else {
			first
		};
	}
}

#[cfg(test)]
#[path = "tests_selection.rs"]
mod tests;
