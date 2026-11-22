use super::*;
use crate::components::{
	ComponentData, ComponentId, ComponentRecord, NO_MESH_PATH, PlayerControllerSettings,
};
use crate::{Key, input::GameInputBinding, project::ProjectData};

pub(in crate::editor) fn create_player(
	data: &mut SceneData,
	project: &mut ProjectData,
) -> Result<EntityKey, String> {
	if data.entities.len() >= MAX_SCENE_ENTITIES {
		return Err("Scene entity limit reached; remove entities before creating a Player".into());
	}
	let key = fresh_key(data);
	let mut actor = EntityData::new(key.clone(), "Player", NO_MESH_PATH)
		.with_renderable(false)
		.with_transform(TransformData {
			position: [0.0, 0.9, 0.0],
			..Default::default()
		});
	let motor = crate::character_controller::CharacterControllerSettings::default();
	ensure_jump_binding(project, &motor.jump_action)?;
	for (id, data) in [
		(
			"body",
			ComponentData::Collider {
				collider: ColliderData::Box {
					half_extents: [0.35, 0.9, 0.35],
					offset: [0.0; 3],
					interaction: Default::default(),
				},
			},
		),
		(
			"character",
			ComponentData::CharacterController { settings: motor },
		),
		(
			"player",
			ComponentData::PlayerController {
				settings: PlayerControllerSettings::default(),
			},
		),
	] {
		actor.put_component(
			ComponentRecord {
				id: ComponentId::new(id)?,
				schema_version: 1,
				enabled: true,
				data,
			},
			false,
		)?;
	}
	data.entities.push(actor);
	Ok(key)
}

fn ensure_jump_binding(project: &mut ProjectData, jump_action: &str) -> Result<(), String> {
	let profile = project.input_profile.get_or_insert_with(Default::default);
	if profile
		.actions
		.iter()
		.any(|action| action.id == jump_action)
	{
		return Ok(());
	}
	let key = [Key::Space, Key::J].into_iter().find(|key| !profile.contains_key(*key))
		.ok_or("Configure a Jump action in Game Input before creating a Player; Space and J are already assigned")?;
	profile.actions.push(GameInputBinding {
		id: jump_action.into(),
		key,
		animation_parameter: None,
	});
	profile.validate()
}

pub(super) fn set_local_player(
	data: &mut SceneData,
	key: Option<&EntityKey>,
) -> Result<(), String> {
	let target = key
		.map(|key| {
			let index = entity_index(data, key.as_str())?;
			let actor = &data.entities[index];
			if actor
				.player_controller
				.as_ref()
				.is_none_or(|controller| !controller.enabled)
			{
				return Err("Set Local Player requires an enabled Player Controller".into());
			}
			actor.validate_player_controller()?;
			Ok::<_, String>(index)
		})
		.transpose()?;
	for (index, entity) in data.entities.iter_mut().enumerate() {
		if let Some(controller) = &mut entity.player_controller {
			controller.settings.local = Some(index) == target;
		}
	}
	Ok(())
}
