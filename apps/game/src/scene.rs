use crate::model_instance::ModelInstance;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityId(u64);

struct SceneEntry {
	id: EntityId,
	instance: ModelInstance,
}

#[derive(Default)]
pub struct Scene {
	entries: Vec<SceneEntry>,
	next_id: u64,
}

impl Scene {
	pub fn spawn(&mut self, instance: ModelInstance) -> EntityId {
		let id = EntityId(self.next_id);

		self.next_id = self
			.next_id
			.checked_add(1)
			.expect("scene entity IDs exhausted");

		self.entries.push(SceneEntry { id, instance });

		id
	}

	pub fn get_mut(&mut self, id: EntityId) -> Option<&mut ModelInstance> {
		self.entries
			.iter_mut()
			.find(|entry| entry.id == id)
			.map(|entry| &mut entry.instance)
	}

	pub fn remove(&mut self, id: EntityId) -> Option<ModelInstance> {
		let index = self.entries.iter().position(|entry| entry.id == id)?;
		let entry = self.entries.remove(index);

		Some(entry.instance)
	}

	pub fn instances(&self) -> impl Iterator<Item = &ModelInstance> {
		self.entries.iter().map(|entry| &entry.instance)
	}
}