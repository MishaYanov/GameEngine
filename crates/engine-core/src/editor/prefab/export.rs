use super::*;

impl EditorSession {
	pub fn export_prefab(&self) -> Result<PrefabData, String> {
		self.export_prefab_with_slots(BTreeMap::new())
	}
	pub fn export_prefab_with_slots(
		&self,
		reference_slots: BTreeMap<String, EntityKey>,
	) -> Result<PrefabData, String> {
		self.export_prefab_with_slots_at(reference_slots, None)
	}

	/// Export the selected subtrees relative to an included object's world origin,
	/// or their average root position when no pivot is chosen.
	pub fn export_prefab_with_slots_at(
		&self,
		reference_slots: BTreeMap<String, EntityKey>,
		pivot: Option<&EntityKey>,
	) -> Result<PrefabData, String> {
		self.require_edit()?;
		self.require_no_transaction()?;
		for target in reference_slots.values() {
			if !self
				.data
				.entities
				.iter()
				.any(|entity| &entity.key == target)
			{
				return Err(
					"Prefab reference slot must declare an existing external scene entity".into(),
				);
			}
		}
		if self.selection.is_empty() {
			return Err("Select entities to create a prefab".into());
		}
		let data = self.data.as_ref();
		let indices: HashMap<_, _> = data
			.entities
			.iter()
			.enumerate()
			.map(|(index, entity)| (&entity.key, index))
			.collect();
		let mut children = vec![Vec::new(); data.entities.len()];
		for (index, entity) in data.entities.iter().enumerate() {
			if let Some(parent) = &entity.parent {
				children[*indices
					.get(parent)
					.ok_or("Prefab source contains a missing parent")?]
				.push(index);
			}
		}
		let mut pending = self
			.selection
			.iter()
			.map(|key| {
				indices
					.get(key)
					.copied()
					.ok_or("Prefab selection contains a missing entity")
			})
			.collect::<Result<Vec<_>, _>>()?;
		let mut included = vec![false; data.entities.len()];
		let mut count = 0;
		while let Some(index) = pending.pop() {
			if included[index] {
				continue;
			}
			included[index] = true;
			count += 1;
			if count > MAX_PREFAB_ENTITIES {
				return Err("Selected prefab subtree exceeds 4096 entities".into());
			}
			pending.extend_from_slice(&children[index]);
		}
		let world = data.world_matrices()?;
		let explicit_pivot = if let Some(key) = pivot {
			let index = indices
				.get(key)
				.copied()
				.filter(|&index| included[index])
				.ok_or_else(|| {
					format!(
						"Prop pivot {:?} must belong to the reviewed selection",
						key.as_str()
					)
				})?;
			Some(world[index].w_axis.truncate().to_array())
		} else {
			None
		};
		let mut entities = Vec::with_capacity(count);
		for (index, entity) in data.entities.iter().enumerate() {
			if !included[index] {
				continue;
			}
			let mut entity = entity.clone();
			entity.linked_prefab = None;
			if entity
				.parent
				.as_ref()
				.is_some_and(|parent| !included[indices[parent]])
			{
				entity.parent = None;
				entity.transform = TransformData::from_matrix(world[index]).map_err(|error| {
					format!("Cannot use {:?} as a Prop root: {error}", entity.name)
				})?;
			}
			entities.push(entity);
		}
		let pivot = explicit_pivot.unwrap_or_else(|| {
			let roots: Vec<_> = entities
				.iter()
				.filter(|entity| entity.parent.is_none())
				.map(|entity| entity.transform.position)
				.collect();
			let count = roots.len().max(1) as f64;
			std::array::from_fn(|axis| {
				(roots
					.iter()
					.map(|position| position[axis] as f64 / count)
					.sum::<f64>()) as f32
			})
		});
		for entity in &mut entities {
			if entity.parent.is_none() {
				entity.transform.position = (glam::Vec3::from_array(entity.transform.position)
					- glam::Vec3::from_array(pivot))
				.to_array();
			}
		}
		let included: HashSet<_> = entities.iter().map(|entity| entity.key.clone()).collect();
		let behaviors: Vec<_> = self
			.project
			.behaviors_for(&self.active_scene_id)
			.filter(|behavior| included.contains(behavior.entity()))
			.cloned()
			.collect();
		let definitions_used: HashSet<_> = behaviors
			.iter()
			.filter_map(|behavior| match behavior {
				crate::project::BehaviorSpec::Shared { definition, .. } => Some(definition),
				_ => None,
			})
			.collect();
		let definitions = self
			.project
			.behavior_definitions
			.iter()
			.filter(|definition| definitions_used.contains(&definition.id))
			.cloned()
			.collect();
		let scene_settings = self
			.project
			.gameplay_settings
			.as_ref()
			.and_then(|value| value.get("scenes"))
			.and_then(|scenes| scenes.get(self.active_scene_id.as_str()))
			.cloned();
		let mut gameplay = PrefabGameplay {
			source_scene: self.active_scene_id.clone(),
			module: self.project.gameplay_module.clone(),
			bindings: data
				.bindings
				.iter()
				.filter(|(_, key)| included.contains(*key))
				.map(|(role, key)| (role.clone(), key.clone()))
				.collect(),
			behaviors,
			definitions,
			scene_settings: None,
			entity_references: BTreeMap::new(),
			scene_references: BTreeMap::new(),
		};
		if let Some(settings) = scene_settings {
			let references = (self.module_resolver)(&self.project)?.prefab_references(&settings)?;
			if references
				.entities
				.values()
				.any(|key| included.contains(key))
			{
				gameplay.scene_settings = Some(settings);
				gameplay.entity_references = references.entities;
				gameplay.scene_references = references.scenes;
			}
		}
		PrefabData {
			schema_version: 3,
			reference_slots,
			entities,
			gameplay: Some(gameplay),
		}
		.validated()
	}
}
