use super::*;

impl EditorSession {
	pub fn instantiate_prefab(
		&mut self,
		prefab: PrefabData,
		position: [f32; 3],
	) -> Result<crate::command::EditorSnapshotDto, String> {
		self.instantiate_prefab_command(prefab, position)?;
		self.snapshot()
	}

	pub fn instantiate_prefab_command(
		&mut self,
		prefab: PrefabData,
		position: [f32; 3],
	) -> Result<(), String> {
		self.instantiate_prefab_command_with_bindings(prefab, position, &BTreeMap::new())
	}
	pub fn instantiate_prefab_command_with_bindings(
		&mut self,
		prefab: PrefabData,
		position: [f32; 3],
		bindings: &BTreeMap<String, EntityKey>,
	) -> Result<(), String> {
		self.require_edit()?;
		for target in bindings.values() {
			if !self
				.data
				.entities
				.iter()
				.any(|entity| &entity.key == target)
			{
				return Err("Prefab reference binding targets a missing scene entity".into());
			}
		}
		if !position.iter().all(|value| value.is_finite()) {
			return Err("Prefab position must be finite".into());
		}
		let prefab = prefab.validated()?;
		let sources = prefab.expanded_entities()?;
		if self
			.data
			.entities
			.len()
			.checked_add(sources.len())
			.is_none_or(|count| count > crate::scene_data::MAX_SCENE_ENTITIES)
		{
			return Err("Instantiating this prefab would exceed the scene entity limit".into());
		}
		let mut used: HashSet<_> = self
			.data
			.entities
			.iter()
			.map(|entity| entity.key.clone())
			.chain(self.data.lights.iter().map(|light| light.key.clone()))
			.collect();
		let remap: HashMap<_, _> = sources
			.iter()
			.map(|entity| {
				let key = loop {
					let key = EntityKey::generate();
					if used.insert(key.clone()) {
						break key;
					}
				};
				(entity.key.clone(), key)
			})
			.collect();
		let mut entities =
			PrefabData::remap_expanded(&sources, &prefab.reference_slots, &remap, bindings)?;
		let mut selection = Vec::new();
		for entity in &mut entities {
			if entity.parent.is_none() {
				entity.transform.position = (glam::Vec3::from_array(entity.transform.position)
					+ glam::Vec3::from_array(position))
				.to_array();
				if !entity
					.transform
					.position
					.iter()
					.all(|value| value.is_finite())
				{
					return Err(
						"Prefab placement exceeds the supported finite position range".into(),
					);
				}
				selection.push(entity.key.clone());
			}
		}
		let mut next = (*self.data).clone();
		let mut project = (*self.project).clone();
		if let Some(gameplay) = prefab.gameplay {
			for (role, key) in gameplay.bindings {
				if next.bindings.contains_key(&role) {
					return Err(format!(
						"Prefab role '{role}' is already bound. Place the assembly in an empty/new scene or unbind the existing role."
					));
				}
				next.bindings.insert(
					role,
					remap
						.get(&key)
						.ok_or("Unresolved prefab role target")?
						.clone(),
				);
			}
			let mut definition_remap = HashMap::new();
			for mut definition in gameplay.definitions {
				let old = definition.id;
				definition.id = format!("prefab-{}", uuid::Uuid::new_v4());
				definition_remap.insert(old, definition.id.clone());
				project.behavior_definitions.push(definition);
			}
			for mut behavior in gameplay.behaviors {
				match &mut behavior {
					crate::project::BehaviorSpec::ControlledMove { entity, .. }
					| crate::project::BehaviorSpec::Spin { entity, .. }
					| crate::project::BehaviorSpec::AcceptAction { entity, .. } => {
						*entity = remap
							.get(entity)
							.ok_or("Unresolved prefab behavior target")?
							.clone()
					}
					crate::project::BehaviorSpec::Shared { entity, definition } => {
						*entity = remap
							.get(entity)
							.ok_or("Unresolved prefab behavior target")?
							.clone();
						*definition = definition_remap
							.get(definition)
							.ok_or("Unresolved prefab shared definition")?
							.clone();
					}
				}
				project.behaviors.push(crate::project::SceneBehavior {
					scene_id: self.active_scene_id.clone(),
					behavior,
				});
			}
			if let Some(mut settings) = gameplay.scene_settings {
				if gameplay.module != project.gameplay_module {
					return Err(
						"Prefab gameplay module differs from this project's selected module".into(),
					);
				}
				let declared = (self.module_resolver)(&project)?.prefab_references(&settings)?;
				if declared.entities != gameplay.entity_references
					|| declared.scenes != gameplay.scene_references
				{
					return Err("Prefab settings references differ from the native module's declared schema".into());
				}
				for (pointer, key) in gameplay.entity_references {
					let target = remap.get(&key).ok_or_else(|| format!("Prefab has unresolved external entity '{}' at '{pointer}'. Include that entity when saving the prefab.", key.as_str()))?;
					*settings
						.pointer_mut(&pointer)
						.ok_or("Prefab settings pointer is unavailable")? =
						serde_json::Value::String(target.as_str().into());
				}
				for (pointer, id) in gameplay.scene_references {
					let target = if id == gameplay.source_scene {
						&self.active_scene_id
					} else {
						project.scenes.get(&id).map_err(|_| format!("Prefab destination scene '{}' is not in this project. Add that destination or resolve the prefab reference before instantiating.", id.as_str()))?;
						&id
					};
					*settings
						.pointer_mut(&pointer)
						.ok_or("Prefab settings pointer is unavailable")? =
						serde_json::Value::String(target.as_str().into());
				}
				let config = project
					.gameplay_settings
					.get_or_insert_with(|| serde_json::json!({"schema_version":1,"scenes":{}}));
				let scenes = config
					.get_mut("scenes")
					.and_then(|value| value.as_object_mut())
					.ok_or("Prefab settings require the module's scene configuration map")?;
				if scenes.contains_key(self.active_scene_id.as_str()) {
					return Err("This scene already has gameplay settings. Place the assembly in an empty/new scene or clear its existing scene configuration.".into());
				}
				scenes.insert(self.active_scene_id.as_str().into(), settings);
			}
		}
		next.entities.extend(entities);
		self.commit_project(next, project, selection, "Instantiate gameplay prefab")
	}
}
