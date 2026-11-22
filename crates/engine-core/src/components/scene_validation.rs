use super::*;

pub fn validate_scene_components(scene: &SceneData) -> Result<(), String> {
	validate_scene_components_with_slots(scene, &HashSet::new())
}
/// Prefab validation may defer capabilities of explicitly declared external
/// reference slots. Installed scenes always use the strict entry point above.
pub fn validate_scene_components_with_slots(
	scene: &SceneData,
	slots: &HashSet<EntityKey>,
) -> Result<(), String> {
	let keys: HashSet<_> = scene.entities.iter().map(|entity| &entity.key).collect();
	let entities: HashMap<_, _> = scene
		.entities
		.iter()
		.map(|entity| (entity.key.as_str(), entity))
		.collect();
	let mut active = 0usize;
	let mut local_player = None;
	for entity in &scene.entities {
		entity.validate_components()?;
		entity
			.validate_player_controller()
			.map_err(|error| format!("Player Controller on {:?}: {error}", entity.name))?;
		if entity.is_local_player() && local_player.replace(&entity.key).is_some() {
			return Err("A scene can assign local input to only one Player Controller".into());
		}
		for component in &entity.native_behaviors {
			for target in component.behavior.entity_references() {
				if !keys.contains(target) {
					return Err(format!(
						"Native component {} on {} targets missing entity {}",
						component.id.as_str(),
						entity.key.as_str(),
						target.as_str()
					));
				}
			}
			if let NativeBehaviorSpec::StateMachine { definition } = &component.behavior {
				definition.validate_scene(&entities, slots)?;
			}
			if !component.enabled {
				continue;
			}
			active += 1;
			if active > MAX_ACTIVE_NATIVE_BEHAVIORS {
				return Err("Scene exceeds 4096 active native behavior attachments".into());
			}
			let result = (|| {
				match &component.behavior {
					NativeBehaviorSpec::KeyDoor {
						actor,
						key_trigger,
						door,
						key_visual,
					} => {
						let actor = required_collider(&entities, slots, actor, "Key/door actor")?;
						let sensor =
							required_collider(&entities, slots, key_trigger, "Key/door trigger")?;
						if sensor.is_some_and(|collider| !collider.interaction().sensor) {
							return Err("Key/door trigger requires a Sensor collider".into());
						}
						if required_collider(&entities, slots, door, "Door")?
							.is_some_and(|collider| collider.interaction().sensor)
						{
							return Err("Door requires a blocking collider".into());
						}
						if let (Some(actor), Some(sensor)) = (actor, sensor)
							&& !actor.interaction().detects(sensor.interaction())
						{
							return Err("Key/door actor and trigger collision filters must accept each other".into());
						}
						if let Some(visual) = key_visual {
							required_visual(&entities, slots, visual)?;
						}
					}
					NativeBehaviorSpec::Collectible {
						actor,
						trigger,
						visual,
						..
					} => {
						let actor =
							required_collider(&entities, slots, actor, "Collectible actor")?;
						let sensor =
							required_collider(&entities, slots, trigger, "Collectible trigger")?;
						if sensor.is_some_and(|collider| !collider.interaction().sensor) {
							return Err("Collectible trigger requires a Sensor collider".into());
						}
						if let (Some(actor), Some(sensor)) = (actor, sensor)
							&& !actor.interaction().detects(sensor.interaction())
						{
							return Err("Collectible actor and trigger collision filters must accept each other".into());
						}
						if let Some(visual) = visual {
							required_visual(&entities, slots, visual)?;
						}
					}
					NativeBehaviorSpec::TimedSpawner { .. }
					| NativeBehaviorSpec::StateMachine { .. } => {}
				}
				Ok::<(), String>(())
			})();
			result.map_err(|error| {
				format!(
					"Native component {} on {}: {error}",
					component.id.as_str(),
					entity.key.as_str()
				)
			})?;
		}
	}
	Ok(())
}
fn required_collider<'a>(
	entities: &HashMap<&str, &'a EntityData>,
	slots: &HashSet<EntityKey>,
	key: &EntityKey,
	label: &str,
) -> Result<Option<&'a ColliderData>, String> {
	if slots.contains(key) {
		return Ok(None);
	}
	let entity = entities
		.get(key.as_str())
		.ok_or("Native behavior entity reference is missing")?;
	entity
		.collider
		.as_ref()
		.map(Some)
		.ok_or_else(|| format!("{label} requires a collider component"))
}
fn required_visual(
	entities: &HashMap<&str, &EntityData>,
	slots: &HashSet<EntityKey>,
	key: &EntityKey,
) -> Result<(), String> {
	if slots.contains(key) {
		return Ok(());
	}
	if entities
		.get(key.as_str())
		.is_some_and(|entity| entity.has_mesh_component())
	{
		Ok(())
	} else {
		Err("Native behavior visual requires a mesh renderer component".into())
	}
}
