//! CPU command implementations. Input bindings and Angular widgets choose
//! commands; neither participates in their scene mutation semantics.
mod audio_source;
mod camera;
use camera::{
	align_camera, camera_forward, camera_right, create_camera, dolly, frame_selection, orbit,
};
mod entities;
use entities::{duplicate, fresh_key};
mod organization;
mod parenting;
mod player;
pub(super) use player::create_player;
mod sky;
mod transform;
use std::collections::{HashMap, HashSet};
use transform::{
	entity_index, entity_indices, finite_result, finite_scalar, finite_vec, hierarchy, inverse,
	root_pivot, selected_subtree, selection_roots, set_world_targets, snap_transform, snap_vec,
	snapping, transform_from_dto, world_target_locals,
};

use crate::command::{
	EditorCommandDto, SceneBindingTarget, SnapSettingsDto, TransformDto, TransformSpace,
};
use glam::{EulerRot, Mat4, Quat, Vec3};

use crate::{
	Bounds3,
	entity_key::EntityKey,
	scene_data::{
		CameraData, ColliderData, EntityData, MAX_SCENE_ENTITIES, SceneData, TransformData,
	},
};

pub(super) fn apply(
	data: &mut SceneData,
	selection: &mut Vec<EntityKey>,
	snap: &SnapSettingsDto,
	bounds: &HashMap<(String, usize), Bounds3>,
	command: EditorCommandDto,
) -> Result<(), String> {
	match command {
		EditorCommandDto::Rename { key, name } => {
			let index = entity_index(data, &key)?;
			data.entities[index].name = name;
		}
		EditorCommandDto::Spawn { entity } => {
			if data.entities.len() >= MAX_SCENE_ENTITIES {
				return Err(
					"Scene entity limit reached; remove entities before spawning more".into(),
				);
			}
			let parent = entity
				.parent
				.map(|key| {
					let index = entity_index(data, &key)?;
					Ok::<_, String>(data.entities[index].key.clone())
				})
				.transpose()?;
			let key = fresh_key(data);
			let entity = EntityData::new(key.clone(), entity.name, entity.model_path)
				.with_asset_id(entity.asset_id)
				.with_parent(parent)
				.with_transform(transform_from_dto(entity.transform)?)
				.with_mesh_index(entity.mesh_index);
			data.entities.push(entity);
			*selection = vec![key];
		}
		EditorCommandDto::CreateCamera => {
			*selection = vec![create_camera(data)?];
		}
		EditorCommandDto::CreateAudioSource => {
			*selection = vec![audio_source::create_audio_source(data)?];
		}
		EditorCommandDto::CreateSky => *selection = vec![sky::create_sky(data)?],
		EditorCommandDto::SetActiveSky { key } => data.set_active_sky(key)?,
		EditorCommandDto::SetSceneEnvironment {
			ambient_linear,
			sky_exposure_ev,
		} => {
			data.environment.ambient_linear = ambient_linear;
			data.environment.sky_exposure_ev = sky_exposure_ev;
		}
		EditorCommandDto::SetLocalPlayer { key } => player::set_local_player(data, key.as_ref())?,
		EditorCommandDto::AlignCameraToSceneView {
			key,
			position,
			yaw,
			pitch,
		} => {
			align_camera(data, &key, position, yaw, pitch)?;
		}
		EditorCommandDto::SetTransforms {
			edits,
			space,
			snap: enabled,
		} => {
			// A single edit is cheaper to scan; batches share one borrowed lookup.
			let indices = (edits.len() > 1).then(|| entity_indices(data));
			let mut targets = HashMap::with_capacity(edits.len());
			for edit in edits {
				let index = match &indices {
					Some(indices) => indices
						.get(edit.key.as_str())
						.copied()
						.ok_or_else(|| format!("Entity key {:?} does not exist", edit.key))?,
					None => entity_index(data, &edit.key)?,
				};
				let mut transform = transform_from_dto(edit.transform)?;
				if snapping(snap, enabled)? {
					snap_transform(&mut transform, snap);
				}
				if targets.insert(index, transform).is_some() {
					return Err(format!(
						"Transform list contains entity {:?} more than once",
						edit.key
					));
				}
			}
			drop(indices);
			match space {
				TransformSpace::Local => {
					for (index, transform) in targets {
						data.entities[index].transform = transform;
					}
				}
				TransformSpace::World => set_world_targets(
					data,
					targets
						.into_iter()
						.map(|(index, transform)| (index, transform.matrix()))
						.collect(),
				)?,
			}
		}
		EditorCommandDto::TranslateSelection {
			delta,
			snap: enabled,
		} => {
			let mut delta = finite_vec(delta, "Translation")?;
			if snapping(snap, enabled)? {
				delta = snap_vec(delta, snap.translation);
			}
			let hierarchy = hierarchy(data)?;
			let roots = selection_roots(&hierarchy, selection)?;
			if delta == Vec3::ZERO {
				return Ok(());
			}
			let world = data.world_matrices()?;
			let mut positions = Vec::with_capacity(roots.len());
			for index in roots {
				let target = world[index].w_axis.truncate() + delta;
				let position = match hierarchy.parents[index] {
					Some(parent) => inverse(world[parent])?.transform_point3(target),
					None => target,
				};
				positions.push((
					index,
					finite_result(position, "Translated position")?.to_array(),
				));
			}
			for (index, position) in positions {
				data.entities[index].transform.position = position;
			}
		}
		EditorCommandDto::RotateSelection {
			radians,
			space,
			snap: enabled,
		} => {
			let mut radians = finite_vec(radians, "Rotation")?;
			if snapping(snap, enabled)? {
				radians = snap_vec(radians, snap.rotation_radians);
			}
			let hierarchy = hierarchy(data)?;
			let roots = selection_roots(&hierarchy, selection)?;
			if radians == Vec3::ZERO {
				return Ok(());
			}
			let delta = Quat::from_euler(EulerRot::XYZ, radians.x, radians.y, radians.z);
			match space {
				TransformSpace::Local => {
					let mut changes = Vec::with_capacity(roots.len());
					for index in roots {
						let mut transform = data.entities[index].transform.clone();
						let r = transform.rotation;
						let rotation = Quat::from_euler(EulerRot::XYZ, r[0], r[1], r[2]) * delta;
						let (x, y, z) = rotation.to_euler(EulerRot::XYZ);
						transform.rotation = [x, y, z];
						changes.push((index, transform));
					}
					for (index, transform) in changes {
						data.entities[index].transform = transform;
					}
				}
				TransformSpace::World => {
					let world = data.world_matrices()?;
					let pivot = root_pivot(&roots, &world)?;
					let change = Mat4::from_translation(pivot)
						* Mat4::from_quat(delta)
						* Mat4::from_translation(-pivot);
					let locals = world_target_locals(
						data,
						roots
							.into_iter()
							.map(|index| (index, change * world[index]))
							.collect(),
						&world,
						Some(&hierarchy.parents),
					)?;
					for (index, transform) in locals {
						data.entities[index].transform = transform;
					}
				}
			}
		}
		EditorCommandDto::ScaleSelection {
			factor,
			snap: enabled,
		} => {
			let factor = finite_vec(factor, "Scale factor")?;
			let use_snap = snapping(snap, enabled)?;
			let hierarchy = hierarchy(data)?;
			let roots = selection_roots(&hierarchy, selection)?;
			for index in roots {
				let mut scale = Vec3::from_array(data.entities[index].transform.scale) * factor;
				if use_snap {
					scale = snap_vec(scale, snap.scale);
				}
				if !scale.is_finite() {
					return Err("Scale overflows the supported finite range".into());
				}
				data.entities[index].transform.scale = scale.to_array();
			}
		}
		EditorCommandDto::DuplicateSelection { offset } => {
			duplicate(data, selection, finite_vec(offset, "Duplicate offset")?)?
		}
		EditorCommandDto::DeleteSelection => {
			let hierarchy = hierarchy(data)?;
			let roots = selection_roots(&hierarchy, selection)?;
			let selected = selected_subtree(&hierarchy, &roots);
			let removed: HashSet<_> = selected
				.into_iter()
				.map(|index| data.entities[index].key.clone())
				.collect();
			data.entities
				.retain(|entity| !removed.contains(&entity.key));
			data.organization
				.root_membership
				.retain(|key, _| !removed.contains(key));
			data.bindings.retain(|_, key| !removed.contains(key));
			data.remove_camera_targets(&removed);
			selection.clear();
		}
		EditorCommandDto::Reparent {
			key,
			parent,
			keep_world,
		} => {
			let key = EntityKey::new(key)?;
			let parent = parent.map(EntityKey::new).transpose()?;
			parenting::reparent_roots(data, &[key], parent.as_ref(), keep_world)?;
		}
		EditorCommandDto::ReparentObjects {
			keys,
			parent,
			keep_world,
		} => parenting::reparent_roots(data, &keys, parent.as_ref(), keep_world)?,
		EditorCommandDto::CreateFolder { name, parent } => {
			organization::create(&mut data.organization, name, parent)?
		}
		EditorCommandDto::RenameFolder { id, name } => {
			organization::rename(&mut data.organization, &id, name)?
		}
		EditorCommandDto::MoveFolder { id, parent } => {
			organization::move_folder(&mut data.organization, &id, parent)?
		}
		EditorCommandDto::MoveRootsToFolder { roots, folder } => {
			organization::move_roots(data, roots, folder)?
		}
		EditorCommandDto::RemoveFolder { id } => organization::remove(&mut data.organization, &id)?,
		EditorCommandDto::SetMaterial { key, material } => {
			let index = entity_index(data, &key)?;
			data.entities[index].material = material;
		}
		EditorCommandDto::SetCollider { key, collider } => {
			let index = entity_index(data, &key)?;
			if let Some(collider) = &collider {
				collider.validate()?;
			}
			if collider.is_some() && data.entities[index].collider.is_none() {
				data.entities[index].component_ids.insert(
					crate::components::COLLIDER_TYPE.into(),
					crate::components::ComponentId::generate(),
				);
			}
			if collider.is_none() {
				data.entities[index]
					.component_ids
					.remove(crate::components::COLLIDER_TYPE);
				data.entities[index].collision_enabled = true;
			}
			data.entities[index].collider = collider;
		}
		EditorCommandDto::SetColliders { updates } => {
			let targets = updates
				.iter()
				.map(|update| {
					let index = entity_index(data, &update.key)?;
					if data.entities[index].collider.is_none() {
						return Err(format!(
							"Entity {:?} needs an existing collider",
							update.key
						));
					}
					Ok(index)
				})
				.collect::<Result<Vec<_>, String>>()?;
			for (index, update) in targets.into_iter().zip(updates) {
				data.entities[index].collider = Some(update.collider);
			}
		}
		EditorCommandDto::AddComponent { key, component } => {
			let index = entity_index(data, &key)?;
			data.entities[index].put_component(component, false)?;
		}
		EditorCommandDto::ReplaceComponent { key, component } => {
			let index = entity_index(data, &key)?;
			data.entities[index].put_component(component, true)?;
			data.clear_disabled_game_camera();
		}
		EditorCommandDto::RemoveComponent { key, component_id } => {
			let index = entity_index(data, &key)?;
			data.entities[index].remove_component(&component_id)?;
			data.clear_disabled_game_camera();
		}
		EditorCommandDto::SetComponentEnabled {
			key,
			component_id,
			enabled,
		} => {
			let index = entity_index(data, &key)?;
			data.entities[index].set_component_enabled(&component_id, enabled)?;
			data.clear_disabled_game_camera();
		}
		EditorCommandDto::UpsertLight { light } => {
			if let Some(existing) = data.lights.iter_mut().find(|entry| entry.key == light.key) {
				*existing = light;
			} else {
				data.lights.push(light);
			}
		}
		EditorCommandDto::RemoveLight { key } => {
			let index = data
				.lights
				.iter()
				.position(|light| light.key.as_str() == key)
				.ok_or_else(|| format!("Light key {key:?} does not exist"))?;
			data.lights.remove(index);
		}
		EditorCommandDto::SetCamera { camera } => data.set_authored_game_camera(camera)?,
		EditorCommandDto::SetDefaultGameCamera { key } => data.select_game_camera(key)?,
		EditorCommandDto::FrameSelection => frame_selection(data, selection, bounds)?,
		EditorCommandDto::OrbitCamera { yaw, pitch } => orbit(data, selection, bounds, yaw, pitch)?,
		EditorCommandDto::PanCamera { right, up } => {
			finite_scalar(right, "Camera pan")?;
			finite_scalar(up, "Camera pan")?;
			let forward = camera_forward(&data.camera);
			let horizontal = camera_right(&data.camera);
			let vertical = horizontal.cross(forward);
			let position =
				Vec3::from_array(data.camera.position) + horizontal * right + vertical * up;
			let mut camera = data.camera.clone();
			camera.position = finite_result(position, "Camera pan")?.to_array();
			data.set_authored_game_camera(camera)?;
		}
		EditorCommandDto::DollyCamera { distance } => dolly(data, selection, bounds, distance)?,
		EditorCommandDto::SetSceneBinding { name, target, .. } => match target {
			SceneBindingTarget::Assign { key } => {
				let index = entity_index(data, &key)?;
				let entity_key = data.entities[index].key.clone();
				data.bindings.insert(name, entity_key);
			}
			SceneBindingTarget::Clear {} => {
				data.bindings.remove(&name);
			}
		},
		_ => return Err("This command is a session operation rather than a scene mutation".into()),
	}
	data.clear_unavailable_sky();
	Ok(())
}

#[cfg(test)]
#[path = "../../tests/unit/editor/operations.rs"]
mod tests;
