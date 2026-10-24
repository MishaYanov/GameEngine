use crate::{
	EntityKey,
	project::ProjectData,
	scene_catalog::SceneId,
	scene_data::{CameraData, EntityData, LightData, SceneData},
};
use std::{
	collections::{HashMap, HashSet, VecDeque},
	mem::size_of,
};
const MAX_HISTORY_BYTES: usize = 16 * 1024 * 1024;
const MAX_HISTORY_RECORDS: usize = 128;
// Both stacks may retain their allocated slots after records move between
// them. Reserve space for those slots as well as each patch's owned payload.
const HISTORY_PAYLOAD_BUDGET: usize =
	MAX_HISTORY_BYTES - 2 * MAX_HISTORY_RECORDS * size_of::<Patch>();
#[derive(Clone)]
struct Change {
	key: EntityKey,
	before: Option<(usize, EntityData)>,
	after: Option<(usize, EntityData)>,
}
pub(super) struct Patch {
	scene_id: SceneId,
	changes: Vec<Change>,
	project: Option<(ProjectData, ProjectData)>,
	camera: Option<(CameraData, CameraData)>,
	lights: Option<(Vec<LightData>, Vec<LightData>)>,
	bindings: Option<(
		std::collections::BTreeMap<String, EntityKey>,
		std::collections::BTreeMap<String, EntityKey>,
	)>,
	selection: (Vec<EntityKey>, Vec<EntityKey>),
	bytes: usize,
	_label: String,
}
impl Patch {
	pub fn in_scene(mut self, id: SceneId) -> Self {
		self.bytes = self.bytes.saturating_add(id.as_str().len());
		self.scene_id = id;
		self
	}
	pub fn between(
		before: &SceneData,
		after: &SceneData,
		project_before: &ProjectData,
		project_after: &ProjectData,
		selection_before: &[EntityKey],
		selection_after: &[EntityKey],
		label: String,
	) -> Result<Self, String> {
		let old: HashMap<_, _> = before
			.entities
			.iter()
			.enumerate()
			.map(|(i, e)| (&e.key, (i, e)))
			.collect();
		let new: HashMap<_, _> = after
			.entities
			.iter()
			.enumerate()
			.map(|(i, e)| (&e.key, (i, e)))
			.collect();
		let mut changes = Vec::new();
		for (index, entity) in before.entities.iter().enumerate() {
			let next = new.get(&entity.key);
			if next.is_none_or(|(_, next)| *next != entity) {
				changes.push(Change {
					key: entity.key.clone(),
					before: Some((index, entity.clone())),
					after: next.map(|(i, e)| (*i, (*e).clone())),
				});
			}
		}
		for (index, entity) in after.entities.iter().enumerate() {
			if !old.contains_key(&entity.key) {
				changes.push(Change {
					key: entity.key.clone(),
					before: None,
					after: Some((index, entity.clone())),
				});
			}
		}
		let camera =
			(before.camera != after.camera).then(|| (before.camera.clone(), after.camera.clone()));
		let lights =
			(before.lights != after.lights).then(|| (before.lights.clone(), after.lights.clone()));
		let bindings = (before.bindings != after.bindings)
			.then(|| (before.bindings.clone(), after.bindings.clone()));
		// Count owned Rust storage, including the two selection snapshots and
		// heap strings. JSON length alone undercounts these allocations.
		let mut bytes = size_of::<Self>()
			.saturating_add(changes.capacity().saturating_mul(size_of::<Change>()))
			.saturating_add(label.capacity());
		for change in &changes {
			bytes = bytes.saturating_add(change.key.as_str().len());
			for (_, entity) in change.before.iter().chain(change.after.iter()) {
				bytes = bytes
					.saturating_add(entity.key.as_str().len())
					.saturating_add(entity.name.capacity())
					.saturating_add(entity.model_path.capacity())
					.saturating_add(entity.parent.as_ref().map_or(0, |key| key.as_str().len()));
			}
		}
		for selection in [selection_before, selection_after] {
			bytes = bytes.saturating_add(selection.len().saturating_mul(size_of::<EntityKey>()));
			for key in selection {
				bytes = bytes.saturating_add(key.as_str().len());
			}
		}
		if let Some((a, b)) = &lights {
			for lights in [a, b] {
				bytes =
					bytes.saturating_add(lights.capacity().saturating_mul(size_of::<LightData>()));
				for light in lights {
					bytes = bytes
						.saturating_add(light.key.as_str().len())
						.saturating_add(light.name.capacity());
				}
			}
		}
		if let Some((a, b)) = &bindings {
			for bindings in [a, b] {
				bytes = bytes.saturating_add(1024);
				// Conservative allowance for BTree node pointers and slack.
				for (name, key) in bindings {
					bytes = bytes
						.saturating_add(256)
						.saturating_add(name.capacity())
						.saturating_add(key.as_str().len());
				}
			}
		}
		let project = (project_before != project_after)
			.then(|| (project_before.clone(), project_after.clone()));
		if let Some((a, b)) = &project {
			for project in [a, b] {
				bytes = bytes
					.saturating_add(project.name.capacity())
					.saturating_add(project.to_json().map_or(0, |s| s.len().saturating_mul(4)))
					.saturating_add(
						project
							.behaviors
							.capacity()
							.saturating_mul(size_of::<crate::project::BehaviorSpec>()),
					);
				for behavior in &project.behaviors {
					bytes = bytes.saturating_add(behavior.entity().as_str().len());
				}
			}
		}
		Ok(Self {
			scene_id: project_before.startup_scene_id.clone(),
			project,
			changes,
			camera,
			lights,
			bindings,
			selection: (selection_before.to_vec(), selection_after.to_vec()),
			bytes,
			_label: label,
		})
	}
	fn empty(&self) -> bool {
		self.changes.is_empty()
			&& self.project.is_none()
			&& self.camera.is_none()
			&& self.lights.is_none()
			&& self.bindings.is_none()
	}
	fn apply(
		&self,
		current: &SceneData,
		current_project: &ProjectData,
		forward: bool,
	) -> Result<(SceneData, ProjectData, Vec<EntityKey>), String> {
		let indices: HashMap<_, _> = current
			.entities
			.iter()
			.enumerate()
			.map(|(index, entity)| (&entity.key, (index, entity)))
			.collect();
		let affected: HashSet<_> = self.changes.iter().map(|change| &change.key).collect();
		let mut restored = Vec::new();
		for change in &self.changes {
			let expected = if forward {
				&change.before
			} else {
				&change.after
			};
			match (expected, indices.get(&change.key)) {
				(None, None) => {}
				(Some((index, entity)), Some((actual_index, actual)))
					if index == actual_index && entity == *actual => {}
				_ => return Err("Undo state no longer matches the recorded entity change".into()),
			}
			if let Some(value) = if forward {
				&change.after
			} else {
				&change.before
			} {
				restored.push(value.clone());
			}
		}
		restored.sort_by_key(|(index, _)| *index);
		let mut survivors = current
			.entities
			.iter()
			.filter(|entity| !affected.contains(&entity.key));
		let count = current.entities.len()
			- self
				.changes
				.iter()
				.filter(|change| indices.contains_key(&change.key))
				.count() + restored.len();
		let mut entities = Vec::with_capacity(count);
		let mut restored = restored.into_iter().peekable();
		for index in 0..count {
			if restored.peek().is_some_and(|(slot, _)| *slot == index) {
				entities.push(restored.next().expect("peeked restoration exists").1);
			} else {
				entities.push(
					survivors
						.next()
						.ok_or("Undo entity ordering is inconsistent")?
						.clone(),
				);
			}
		}
		if restored.next().is_some() || survivors.next().is_some() {
			return Err("Undo entity ordering is inconsistent".into());
		}
		let mut next = SceneData {
			schema_version: current.schema_version,
			bindings: current.bindings.clone(),
			entities,
			camera: current.camera.clone(),
			lights: current.lights.clone(),
		};
		if let Some((a, b)) = &self.camera {
			if &current.camera != if forward { a } else { b } {
				return Err("Undo camera state no longer matches".into());
			}
			next.camera = if forward { b } else { a }.clone();
		}
		if let Some((a, b)) = &self.lights {
			if &current.lights != if forward { a } else { b } {
				return Err("Undo lighting state no longer matches".into());
			}
			next.lights = if forward { b } else { a }.clone();
		}
		if let Some((a, b)) = &self.bindings {
			if &current.bindings != if forward { a } else { b } {
				return Err("Undo bindings no longer match".into());
			}
			next.bindings = if forward { b } else { a }.clone();
		}
		next.validate()?;
		let selection = if forward {
			&self.selection.1
		} else {
			&self.selection.0
		};
		let keys: HashSet<_> = next.entities.iter().map(|entity| &entity.key).collect();
		if selection.len() > 4096 || selection.iter().any(|key| !keys.contains(key)) {
			return Err("Undo selection is inconsistent with the restored scene".into());
		}
		let project = if let Some((a, b)) = &self.project {
			if current_project != if forward { a } else { b } {
				return Err("Undo project configuration no longer matches".into());
			}
			if forward { b } else { a }
		} else {
			current_project
		};
		project.validate_scene_for(&self.scene_id, &next)?;
		Ok((next, project.clone(), selection.clone()))
	}
}
#[derive(Default)]
pub(super) struct History {
	undo: VecDeque<Patch>,
	redo: Vec<Patch>,
	bytes: usize,
}
impl History {
	pub fn target_scene(&self, redo: bool) -> Option<&SceneId> {
		if redo {
			self.redo.last()
		} else {
			self.undo.back()
		}
			.map(|p| &p.scene_id)
	}
	pub fn retained_scene_ids(&self) -> HashSet<SceneId> {
		let mut ids = HashSet::new();
		for patch in self.undo.iter().chain(self.redo.iter()) {
			ids.insert(patch.scene_id.clone());
			if let Some((a, b)) = &patch.project {
				ids.extend(
					a.scenes
						.iter()
						.chain(b.scenes.iter())
						.map(|s| s.id().clone()),
				);
			}
		}
		ids
	}
	pub fn preview_restore(
		&self,
		current: &SceneData,
		project: &ProjectData,
		redo: bool,
	) -> Result<Option<(SceneData, ProjectData, Vec<EntityKey>)>, String> {
		let record = if redo {
			self.redo.last()
		} else {
			self.undo.back()
		};
		record.map(|r| r.apply(current, project, redo)).transpose()
	}
	pub fn retain_entity_keys(&self, keys: &mut HashSet<EntityKey>) {
		for patch in self.undo.iter().chain(self.redo.iter()) {
			for change in &patch.changes {
				keys.insert(change.key.clone());
				for (_, entity) in change.before.iter().chain(change.after.iter()) {
					if let Some(parent) = &entity.parent {
						keys.insert(parent.clone());
					}
				}
			}
		}
	}
	pub fn can_undo(&self) -> bool {
		!self.undo.is_empty()
	}
	pub fn can_redo(&self) -> bool {
		!self.redo.is_empty()
	}
	pub fn record(&mut self, patch: Patch) -> Result<(), String> {
		if patch.empty() {
			return Ok(());
		}
		if patch.bytes > HISTORY_PAYLOAD_BUDGET {
			return Err("Edit exceeds the 16 MiB undo budget; split the operation".into());
		}
		for discarded in self.redo.drain(..) {
			self.bytes -= discarded.bytes;
		}
		// Evict before pushing, so a full 128-slot deque never grows to 256
		// merely to discard its oldest record immediately afterwards.
		while self.undo.len() + self.redo.len() >= MAX_HISTORY_RECORDS
			|| self.bytes + patch.bytes > HISTORY_PAYLOAD_BUDGET
		{
			self.bytes -= self
				.undo
				.pop_front()
				.expect("history has a record to evict")
				.bytes;
		}
		self.bytes += patch.bytes;
		self.undo.push_back(patch);
		Ok(())
	}
	pub fn restore(
		&mut self,
		current: &SceneData,
		current_project: &ProjectData,
		redo: bool,
	) -> Result<Option<(SceneData, ProjectData, Vec<EntityKey>)>, String> {
		let record = if redo {
			self.redo.last()
		} else {
			self.undo.back()
		};
		let Some(record) = record else {
			return Ok(None);
		};
		let next = record.apply(current, current_project, redo)?;
		if redo {
			self.undo.push_back(self.redo.pop().expect("record exists"));
		} else {
			self.redo.push(self.undo.pop_back().expect("record exists"));
		}
		Ok(Some(next))
	}
}
