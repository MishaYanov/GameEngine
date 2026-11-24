use super::*;

impl EntityData {
	pub fn component_retained_bytes(&self) -> usize {
		let mut bytes = self.component_ids.len().saturating_mul(128);
		if let Some(mesh_id) = &self.mesh_id {
			bytes = bytes.saturating_add(mesh_id.as_str().len());
		}
		if let Some(graph) = self
			.material
			.as_ref()
			.and_then(|material| material.source_graph.as_deref())
		{
			bytes = bytes.saturating_add(graph.retained_bytes());
		}
		if let Some(emitter) = &self.particle_emitter {
			bytes = bytes.saturating_add(emitter.id.as_str().len());
		}
		if let Some(light) = &self.light {
			bytes = bytes
				.saturating_add(std::mem::size_of_val(light.as_ref()))
				.saturating_add(light.id.as_str().len());
		}
		if let Some(sky) = &self.sky {
			bytes = bytes
				.saturating_add(std::mem::size_of_val(sky.as_ref()))
				.saturating_add(sky.id.as_str().len());
			if let Some(image) = sky.settings.background.image() {
				bytes = bytes.saturating_add(image.as_str().len());
			}
		}
		if let Some(source) = &self.audio_source {
			bytes = bytes
				.saturating_add(std::mem::size_of_val(source.as_ref()))
				.saturating_add(source.id.as_str().len());
			if let Some(clip) = &source.settings.clip {
				bytes = bytes.saturating_add(clip.asset.as_str().len());
			}
		}
		if let Some(controller) = &self.player_controller {
			bytes = bytes
				.saturating_add(controller.id.as_str().len())
				.saturating_add(std::mem::size_of_val(controller.as_ref()));
		}
		if let Some(camera) = &self.camera {
			bytes = bytes
				.saturating_add(camera.id.as_str().len())
				.saturating_add(std::mem::size_of_val(camera.as_ref()));
			if let Some(target) = camera.settings.rig.target() {
				bytes = bytes.saturating_add(target.as_str().len());
			}
		}
		if let Some(component) = &self.character_controller {
			bytes = bytes
				.saturating_add(component.id.as_str().len())
				.saturating_add(component.settings.jump_action.capacity());
		}
		if let Some(component) = &self.animation {
			bytes = bytes
				.saturating_add(component.id.as_str().len())
				.saturating_add(component.settings.retained_bytes());
		}
		if let Some(component) = &self.animator {
			bytes = bytes
				.saturating_add(component.id.as_str().len())
				.saturating_add(std::mem::size_of_val(component.as_ref()));
			if let crate::animation::AnimatorBinding::Visual { targets } =
				&component.settings.binding
			{
				bytes = bytes.saturating_add(targets.len().saturating_mul(640));
			}
		}
		if let Some(component) = &self.interactable {
			bytes = bytes
				.saturating_add(component.id.as_str().len())
				.saturating_add(component.settings.action.capacity());
		}
		for (kind, id) in &self.component_ids {
			bytes = bytes
				.saturating_add(kind.capacity())
				.saturating_add(id.as_str().len());
		}
		bytes = bytes.saturating_add(
			self.native_behaviors
				.capacity()
				.saturating_mul(std::mem::size_of::<NativeBehaviorComponent>()),
		);
		for component in &self.native_behaviors {
			bytes = bytes.saturating_add(component.id.as_str().len());
			if let NativeBehaviorSpec::StateMachine { definition } = &component.behavior {
				bytes = bytes.saturating_add(definition.retained_bytes());
			}
			for key in component.behavior.entity_references() {
				bytes = bytes.saturating_add(key.as_str().len());
			}
			if let NativeBehaviorSpec::TimedSpawner {
				prefab_path,
				bindings,
				..
			} = &component.behavior
			{
				bytes = bytes
					.saturating_add(prefab_path.capacity())
					.saturating_add(bindings.len().saturating_mul(128));
				for name in bindings.keys() {
					bytes = bytes.saturating_add(name.capacity());
				}
			}
		}
		for script in &self.scripts {
			bytes = bytes
				.saturating_add(size_of::<ScriptComponent>())
				.saturating_add(script.id.as_str().len())
				.saturating_add(script.settings.retained_bytes());
		}
		bytes
	}
	pub fn has_mesh_component(&self) -> bool {
		self.model_path != NO_MESH_PATH
	}
	pub fn component_id(&self, type_id: &str) -> ComponentId {
		self.component_ids.get(type_id).cloned().unwrap_or_else(|| {
			ComponentId::new(if type_id == MESH_TYPE {
				"mesh"
			} else {
				"collider"
			})
			.expect("fixed migrated component ID")
		})
	}
	pub fn component_records(&self) -> Vec<ComponentRecord> {
		let mut records = Vec::with_capacity(2 + self.native_behaviors.len() + self.scripts.len());
		if let Some(emitter) = &self.particle_emitter {
			records.push(ComponentRecord {
				id: emitter.id.clone(),
				schema_version: 1,
				enabled: emitter.enabled,
				data: ComponentData::ParticleEmitter {
					settings: emitter.settings,
				},
			});
		}
		if let Some(light) = &self.light {
			records.push(ComponentRecord {
				id: light.id.clone(),
				schema_version: 1,
				enabled: light.enabled,
				data: ComponentData::Light {
					settings: light.settings,
				},
			});
		}
		if let Some(sky) = &self.sky {
			records.push(ComponentRecord {
				id: sky.id.clone(),
				schema_version: 1,
				enabled: sky.enabled,
				data: ComponentData::Sky {
					settings: sky.settings.clone(),
				},
			});
		}
		if let Some(source) = &self.audio_source {
			records.push(ComponentRecord {
				id: source.id.clone(),
				schema_version: 1,
				enabled: source.enabled,
				data: ComponentData::AudioSource {
					settings: source.settings.clone(),
				},
			});
		}
		if let Some(controller) = &self.player_controller {
			records.push(ComponentRecord {
				id: controller.id.clone(),
				schema_version: 1,
				enabled: controller.enabled,
				data: ComponentData::PlayerController {
					settings: controller.settings.clone(),
				},
			});
		}
		if let Some(camera) = &self.camera {
			records.push(ComponentRecord {
				id: camera.id.clone(),
				schema_version: 1,
				enabled: camera.enabled,
				data: ComponentData::Camera {
					settings: camera.settings.clone(),
				},
			});
		}
		if let Some(component) = &self.character_controller {
			records.push(ComponentRecord {
				id: component.id.clone(),
				schema_version: 1,
				enabled: component.enabled,
				data: ComponentData::CharacterController {
					settings: component.settings.clone(),
				},
			});
		}
		if let Some(component) = &self.animation {
			records.push(ComponentRecord {
				id: component.id.clone(),
				schema_version: 1,
				enabled: component.enabled,
				data: ComponentData::Animation {
					settings: component.settings.clone(),
				},
			});
		}
		if let Some(component) = &self.animator {
			records.push(ComponentRecord {
				id: component.id.clone(),
				schema_version: 1,
				enabled: component.enabled,
				data: ComponentData::Animator {
					settings: component.settings.clone(),
				},
			});
		}
		if let Some(component) = &self.interactable {
			records.push(ComponentRecord {
				id: component.id.clone(),
				schema_version: 1,
				enabled: component.enabled,
				data: ComponentData::Interactable {
					settings: component.settings.clone(),
				},
			});
		}
		if self.has_mesh_component() {
			records.push(ComponentRecord {
				id: self.component_id(MESH_TYPE),
				schema_version: 1,
				enabled: self.renderable,
				data: ComponentData::MeshRenderer {
					model_path: self.model_path.clone(),
					asset_id: self.asset_id.clone(),
					mesh_index: self.mesh_index,
					mesh_id: self.mesh_id.clone(),
					material: self.material.clone(),
				},
			});
		}
		if let Some(collider) = &self.collider {
			records.push(ComponentRecord {
				id: self.component_id(COLLIDER_TYPE),
				schema_version: 1,
				enabled: self.collision_enabled,
				data: ComponentData::Collider {
					collider: collider.clone(),
				},
			});
		}
		records.extend(
			self.native_behaviors
				.iter()
				.map(|component| ComponentRecord {
					id: component.id.clone(),
					schema_version: 1,
					enabled: component.enabled,
					data: ComponentData::NativeBehavior {
						behavior: component.behavior.clone(),
					},
				}),
		);
		records.extend(self.scripts.iter().map(|script| ComponentRecord {
			id: script.id.clone(),
			schema_version: 1,
			enabled: script.enabled,
			data: ComponentData::Script {
				settings: script.settings.clone(),
			},
		}));
		records
	}
	pub fn validate_components(&self) -> Result<(), String> {
		if self.native_behaviors.len() > MAX_NATIVE_BEHAVIORS_PER_OBJECT
			|| self.scripts.len() > MAX_SCRIPTS_PER_OBJECT
			|| self.component_records().len() > MAX_COMPONENTS_PER_OBJECT
		{
			return Err("Object component count exceeds its limit".into());
		}
		for (type_id, id) in &self.component_ids {
			if (type_id != MESH_TYPE && type_id != COLLIDER_TYPE)
				|| (type_id == MESH_TYPE && !self.has_mesh_component())
				|| (type_id == COLLIDER_TYPE && self.collider.is_none())
			{
				return Err(
					"Component identity metadata refers to an absent or unsupported singleton"
						.into(),
				);
			}
			ComponentId::new(id.as_str())?;
		}
		let mut ids = HashSet::new();
		for component in self.component_records() {
			component.validate()?;
			if !ids.insert(component.id) {
				return Err("Component IDs must be unique within an object".into());
			}
		}
		Ok(())
	}
	pub fn put_component(
		&mut self,
		component: ComponentRecord,
		replace: bool,
	) -> Result<(), String> {
		component.validate()?;
		let records = self.component_records();
		let existing = records.iter().find(|old| old.id == component.id);
		if replace {
			if existing.is_none_or(|old| old.type_id() != component.type_id()) {
				return Err(
					"Replace requires an existing component of the same type and ID".into(),
				);
			}
		} else if existing.is_some() {
			return Err("Component ID is already attached".into());
		}
		let type_id = component.type_id();
		if !matches!(type_id, NATIVE_TYPE | SCRIPT_TYPE)
			&& !replace
			&& records.iter().any(|old| old.type_id() == type_id)
		{
			return Err("This component type permits one attachment per object".into());
		}
		if !replace && type_id == SCRIPT_TYPE && self.scripts.len() >= MAX_SCRIPTS_PER_OBJECT {
			return Err("Object script count exceeds its limit".into());
		}
		if !replace && records.len() >= MAX_COMPONENTS_PER_OBJECT {
			return Err("Object component count exceeds its limit".into());
		}
		match component.data {
			ComponentData::ParticleEmitter { settings } => {
				self.particle_emitter = Some(ParticleEmitterComponent {
					id: component.id,
					enabled: component.enabled,
					settings,
				});
			}
			ComponentData::Light { settings } => {
				self.light = Some(Box::new(LightComponent {
					id: component.id,
					enabled: component.enabled,
					settings,
				}));
			}
			ComponentData::Sky { settings } => {
				self.sky = Some(Box::new(SkyComponent {
					id: component.id,
					enabled: component.enabled,
					settings,
				}));
			}
			ComponentData::AudioSource { settings } => {
				self.audio_source = Some(Box::new(AudioSourceComponent {
					id: component.id,
					enabled: component.enabled,
					settings,
				}));
			}
			ComponentData::PlayerController { settings } => {
				self.player_controller = Some(Box::new(PlayerControllerComponent {
					id: component.id,
					enabled: component.enabled,
					settings,
				}));
			}
			ComponentData::Camera { settings } => {
				self.camera = Some(Box::new(crate::game_camera::CameraComponent {
					id: component.id,
					enabled: component.enabled,
					settings,
				}));
			}
			ComponentData::CharacterController { settings } => {
				self.character_controller =
					Some(crate::character_controller::CharacterControllerComponent {
						id: component.id,
						enabled: component.enabled,
						settings,
					})
			}
			ComponentData::Animation { settings } => {
				self.animation = Some(crate::animation::AnimationComponent {
					id: component.id,
					enabled: component.enabled,
					settings,
				})
			}
			ComponentData::Animator { settings } => {
				self.animator = Some(Box::new(crate::animation::AnimatorComponent {
					id: component.id,
					enabled: component.enabled,
					settings,
				}));
			}
			ComponentData::Interactable { settings } => {
				self.interactable = Some(InteractableComponent {
					id: component.id,
					enabled: component.enabled,
					settings,
				});
			}
			ComponentData::MeshRenderer {
				model_path,
				asset_id,
				mesh_index,
				mesh_id,
				material,
			} => {
				self.model_path = model_path;
				self.asset_id = asset_id;
				self.mesh_index = mesh_index;
				self.mesh_id = mesh_id;
				self.material = material;
				self.renderable = component.enabled;
				self.component_ids.insert(MESH_TYPE.into(), component.id);
			}
			ComponentData::Collider { collider } => {
				self.collider = Some(collider);
				self.collision_enabled = component.enabled;
				self.component_ids
					.insert(COLLIDER_TYPE.into(), component.id);
			}
			ComponentData::Script { settings } => {
				let attachment = ScriptComponent {
					id: component.id,
					enabled: component.enabled,
					settings,
				};
				if let Some(old) = self.scripts.iter_mut().find(|old| old.id == attachment.id) {
					*old = attachment;
				} else {
					self.scripts.push(attachment);
				}
			}
			ComponentData::NativeBehavior { behavior } => {
				let attachment = NativeBehaviorComponent {
					id: component.id,
					enabled: component.enabled,
					behavior,
				};
				if let Some(old) = self
					.native_behaviors
					.iter_mut()
					.find(|old| old.id == attachment.id)
				{
					*old = attachment;
				} else {
					self.native_behaviors.push(attachment);
				}
			}
		}
		self.validate_components()
	}
	pub fn remove_component(&mut self, id: &ComponentId) -> Result<(), String> {
		let record = self
			.component_records()
			.into_iter()
			.find(|record| &record.id == id)
			.ok_or("Component does not exist")?;
		match record.data {
			ComponentData::AudioSource { .. } => self.audio_source = None,
			ComponentData::Sky { .. } => self.sky = None,
			ComponentData::Light { .. } => self.light = None,
			ComponentData::ParticleEmitter { .. } => self.particle_emitter = None,
			ComponentData::PlayerController { .. } => self.player_controller = None,
			ComponentData::Camera { .. } => self.camera = None,
			ComponentData::CharacterController { .. } => self.character_controller = None,
			ComponentData::Animation { .. } => self.animation = None,
			ComponentData::Animator { .. } => self.animator = None,
			ComponentData::Interactable { .. } => self.interactable = None,
			ComponentData::MeshRenderer { .. } => {
				self.model_path = NO_MESH_PATH.into();
				self.asset_id = None;
				self.mesh_index = None;
				self.mesh_id = None;
				self.material = None;
				self.renderable = false;
				self.component_ids.remove(MESH_TYPE);
			}
			ComponentData::Collider { .. } => {
				self.collider = None;
				self.collision_enabled = true;
				self.component_ids.remove(COLLIDER_TYPE);
			}
			ComponentData::Script { .. } => self.scripts.retain(|component| &component.id != id),
			ComponentData::NativeBehavior { .. } => self
				.native_behaviors
				.retain(|component| &component.id != id),
		}
		Ok(())
	}
	pub fn set_component_enabled(&mut self, id: &ComponentId, enabled: bool) -> Result<(), String> {
		let mut record = self
			.component_records()
			.into_iter()
			.find(|record| &record.id == id)
			.ok_or("Component does not exist")?;
		record.enabled = enabled;
		if !enabled && let ComponentData::PlayerController { settings } = &mut record.data {
			settings.local = false;
		}
		self.put_component(record, true)
	}
	pub fn remap_component_references(
		&mut self,
		remap: &HashMap<EntityKey, EntityKey>,
	) -> Result<(), String> {
		if let Some(camera) = &mut self.camera {
			camera.settings.rig.remap_target(remap);
		}
		if let Some(animator) = &mut self.animator
			&& let crate::animation::AnimatorBinding::Visual { targets } =
				&mut animator.settings.binding
		{
			for target in targets {
				if let Some(key) = remap.get(&target.entity_key) {
					target.entity_key.clone_from(key);
				}
			}
		}
		for component in &mut self.native_behaviors {
			component.behavior.remap_entity_references(remap);
		}
		for component in &mut self.scripts {
			component.settings.remap_entities(remap);
		}
		self.validate_components()
	}
}
