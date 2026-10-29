//! CPU command implementations. Input bindings and Angular widgets choose
//! commands; neither participates in their scene mutation semantics.
use std::collections::{HashMap, HashSet};

use crate::command::{EditorCommandDto, SnapSettingsDto, TransformDto, TransformSpace};
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
				.with_parent(parent)
				.with_transform(transform_from_dto(entity.transform)?)
				.with_mesh_index(entity.mesh_index);
			data.entities.push(entity);
			*selection = vec![key];
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
			data.bindings.retain(|_, key| !removed.contains(key));
			selection.clear();
		}
		EditorCommandDto::Reparent {
			key,
			parent,
			keep_world,
		} => reparent(data, &key, parent.as_deref(), keep_world)?,
		EditorCommandDto::SetMaterial { key, material } => {
			let index = entity_index(data, &key)?;
			data.entities[index].material = material;
		}
		EditorCommandDto::SetCollider { key, collider } => {
			let index = entity_index(data, &key)?;
			if let Some(collider) = &collider {
				collider.validate()?;
			}
			data.entities[index].collider = collider;
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
		EditorCommandDto::SetCamera { camera } => {
			camera.validate()?;
			data.camera = camera;
		}
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
			data.camera.position = finite_result(position, "Camera pan")?.to_array();
		}
		EditorCommandDto::DollyCamera { distance } => dolly(data, selection, bounds, distance)?,
		_ => return Err("This command is a session operation rather than a scene mutation".into()),
	}
	Ok(())
}

fn entity_index(data: &SceneData, key: &str) -> Result<usize, String> {
	data.entities
		.iter()
		.position(|entity| entity.key.as_str() == key)
		.ok_or_else(|| format!("Entity key {key:?} does not exist"))
}

fn entity_indices(data: &SceneData) -> HashMap<&str, usize> {
	let mut indices = HashMap::with_capacity(data.entities.len());
	for (index, entity) in data.entities.iter().enumerate() {
		// Match the single-entity scan even before scene validation rejects duplicates.
		indices.entry(entity.key.as_str()).or_insert(index);
	}
	indices
}

fn finite_scalar(value: f32, label: &str) -> Result<(), String> {
	if value.is_finite() {
		Ok(())
	} else {
		Err(format!("{label} must be finite"))
	}
}

fn finite_vec(value: [f32; 3], label: &str) -> Result<Vec3, String> {
	finite_result(Vec3::from_array(value), label)
}

fn finite_result(value: Vec3, label: &str) -> Result<Vec3, String> {
	if value.is_finite() {
		Ok(value)
	} else {
		Err(format!(
			"{label} must remain within the finite supported range"
		))
	}
}

fn transform_from_dto(value: TransformDto) -> Result<TransformData, String> {
	finite_vec(value.position, "Position")?;
	finite_vec(value.rotation, "Rotation")?;
	finite_vec(value.scale, "Scale")?;
	Ok(TransformData {
		position: value.position,
		rotation: value.rotation,
		scale: value.scale,
	})
}

fn snapping(settings: &SnapSettingsDto, command_enabled: bool) -> Result<bool, String> {
	if command_enabled && settings.enabled {
		for value in [
			settings.translation,
			settings.rotation_radians,
			settings.scale,
		] {
			if !value.is_finite() || value <= 0.0 {
				return Err("Enabled snapping requires positive finite increments".into());
			}
		}
		Ok(true)
	} else {
		Ok(false)
	}
}

fn snap_vec(value: Vec3, increment: f32) -> Vec3 {
	// Divide in f64 so a finite f32 value and small grid increment do not
	// overflow merely while computing the nearest grid coordinate.
	Vec3::from_array(
		value
			.to_array()
			.map(|v| ((v as f64 / increment as f64).round() * increment as f64) as f32),
	)
}

fn snap_transform(transform: &mut TransformData, settings: &SnapSettingsDto) {
	transform.position =
		snap_vec(Vec3::from_array(transform.position), settings.translation).to_array();
	transform.rotation = snap_vec(
		Vec3::from_array(transform.rotation),
		settings.rotation_radians,
	)
	.to_array();
	transform.scale = snap_vec(Vec3::from_array(transform.scale), settings.scale).to_array();
}

struct Hierarchy<'a> {
	indices: HashMap<&'a str, usize>,
	parents: Vec<Option<usize>>,
}

fn hierarchy(data: &SceneData) -> Result<Hierarchy<'_>, String> {
	let indices = entity_indices(data);
	if indices.len() != data.entities.len() {
		return Err("Scene contains duplicate entity keys".into());
	}
	let parents = data
		.entities
		.iter()
		.map(|entity| {
			entity
				.parent
				.as_ref()
				.map(|key| {
					indices
						.get(key.as_str())
						.copied()
						.ok_or_else(|| format!("Parent {:?} does not exist", key.as_str()))
				})
				.transpose()
		})
		.collect::<Result<Vec<_>, _>>()?;
	Ok(Hierarchy { indices, parents })
}

fn selection_roots(
	hierarchy: &Hierarchy<'_>,
	selection: &[EntityKey],
) -> Result<Vec<usize>, String> {
	let Hierarchy { indices, parents } = hierarchy;
	let selected: HashSet<_> = selection
		.iter()
		.map(|key| {
			indices
				.get(key.as_str())
				.copied()
				.ok_or_else(|| format!("Selected entity {:?} no longer exists", key.as_str()))
		})
		.collect::<Result<_, _>>()?;
	if selected.is_empty() {
		return Err("Select at least one entity first".into());
	}
	let mut has_selected_ancestor = vec![false; parents.len()];
	let mut state = vec![0_u8; parents.len()];
	let mut path = Vec::new();
	for index in 0..parents.len() {
		let mut current = Some(index);
		while let Some(node) = current {
			match state[node] {
				2 => break,
				1 => return Err("Scene hierarchy contains a parent cycle".into()),
				_ => {
					state[node] = 1;
					path.push(node);
					current = parents[node];
				}
			}
		}
		while let Some(node) = path.pop() {
			has_selected_ancestor[node] = parents[node]
				.is_some_and(|parent| selected.contains(&parent) || has_selected_ancestor[parent]);
			state[node] = 2;
		}
	}
	Ok((0..parents.len())
		.filter(|index| selected.contains(index) && !has_selected_ancestor[*index])
		.collect())
}

fn selected_subtree(hierarchy: &Hierarchy<'_>, roots: &[usize]) -> Vec<usize> {
	let parents = &hierarchy.parents;
	let mut children = vec![Vec::new(); parents.len()];
	for (index, parent) in parents.iter().enumerate() {
		if let Some(parent) = parent {
			children[*parent].push(index);
		}
	}
	let mut included = vec![false; parents.len()];
	let mut stack = roots.to_vec();
	while let Some(index) = stack.pop() {
		if included[index] {
			continue;
		}
		included[index] = true;
		stack.extend(children[index].iter().copied());
	}
	included
		.into_iter()
		.enumerate()
		.filter_map(|(index, selected)| selected.then_some(index))
		.collect()
}

/// Resolve every desired world target against the resulting parent world,
/// including edited ancestors. Work is iterative and independent of Vec order.
fn set_world_targets(data: &mut SceneData, targets: HashMap<usize, Mat4>) -> Result<(), String> {
	let original = data.world_matrices()?;
	let locals = world_target_locals(data, targets, &original, None)?;
	for (index, transform) in locals {
		data.entities[index].transform = transform;
	}
	Ok(())
}

fn world_target_locals(
	data: &SceneData,
	targets: HashMap<usize, Mat4>,
	original: &[Mat4],
	parents: Option<&[Option<usize>]>,
) -> Result<Vec<(usize, TransformData)>, String> {
	if targets
		.iter()
		.all(|(&index, &matrix)| matrix == original[index])
	{
		return Ok(Vec::new());
	}
	let resolved_hierarchy;
	let parents = match parents {
		Some(parents) => parents,
		None => {
			resolved_hierarchy = hierarchy(data)?;
			&resolved_hierarchy.parents
		}
	};
	let mut resolved = vec![None; data.entities.len()];
	let mut locals = Vec::with_capacity(targets.len());
	let mut path = Vec::new();
	for &index in targets.keys() {
		let mut current = Some(index);
		while let Some(node) = current {
			if resolved[node].is_some() {
				break;
			}
			path.push(node);
			current = parents[node];
		}
		while let Some(node) = path.pop() {
			let parent_world =
				parents[node].map(|parent| resolved[parent].expect("parent resolved before child"));
			let world = if let Some(&target) = targets.get(&node) {
				if !target.is_finite() {
					return Err("World transform must remain finite".into());
				}
				let local = match parent_world {
					Some(parent) => inverse(parent)? * target,
					None => target,
				};
				locals.push((node, TransformData::from_matrix(local)?));
				target
			} else {
				parent_world.map_or(original[node], |parent| {
					parent * data.entities[node].transform.matrix()
				})
			};
			resolved[node] = Some(world);
		}
	}
	Ok(locals)
}

fn inverse(matrix: Mat4) -> Result<Mat4, String> {
	if !matrix.is_finite() || matrix.determinant() == 0.0 {
		return Err("Parent world transform is singular; give its scale nonzero values before editing in world space".into());
	}
	let inverse = matrix.inverse();
	if !inverse.is_finite() {
		return Err(
			"Parent world transform cannot be inverted within the supported finite range".into(),
		);
	}
	Ok(inverse)
}

fn root_pivot(roots: &[usize], world: &[Mat4]) -> Result<Vec3, String> {
	let count = roots.len() as f32;
	finite_result(
		roots.iter().fold(Vec3::ZERO, |sum, &index| {
			sum + world[index].w_axis.truncate() / count
		}),
		"Selection pivot",
	)
}

fn fresh_key(data: &SceneData) -> EntityKey {
	loop {
		let key = EntityKey::generate();
		if !data.entities.iter().any(|entity| entity.key == key)
			&& !data.lights.iter().any(|light| light.key == key)
		{
			return key;
		}
	}
}

fn duplicate(
	data: &mut SceneData,
	selection: &mut Vec<EntityKey>,
	offset: Vec3,
) -> Result<(), String> {
	let hierarchy = hierarchy(data)?;
	let roots = selection_roots(&hierarchy, selection)?;
	let indices = selected_subtree(&hierarchy, &roots);
	let roots: HashSet<_> = roots.into_iter().collect();
	if data
		.entities
		.len()
		.checked_add(indices.len())
		.is_none_or(|count| count > MAX_SCENE_ENTITIES)
	{
		return Err("Duplicating this selection would exceed the scene entity limit".into());
	}
	let original_world = data.world_matrices()?;
	let mut keys = HashMap::with_capacity(indices.len());
	let mut used: HashSet<_> = data
		.entities
		.iter()
		.map(|entity| entity.key.clone())
		.chain(data.lights.iter().map(|light| light.key.clone()))
		.collect();
	for &index in &indices {
		let key = loop {
			let key = EntityKey::generate();
			if used.insert(key.clone()) {
				break key;
			}
		};
		keys.insert(data.entities[index].key.clone(), key);
	}
	let mut copies = Vec::with_capacity(indices.len());
	for index in indices {
		let mut copy = data.entities[index].clone();
		copy.key = keys[&copy.key].clone();
		copy.name.push_str(" Copy");
		copy.parent = copy
			.parent
			.map(|key| keys.get(&key).cloned().unwrap_or(key));
		if roots.contains(&index) && offset != Vec3::ZERO {
			let target = original_world[index].w_axis.truncate() + offset;
			let position = match hierarchy.parents[index] {
				Some(parent) => inverse(original_world[parent])?.transform_point3(target),
				None => target,
			};
			copy.transform.position = finite_result(position, "Duplicated position")?.to_array();
		}
		copies.push(copy);
	}
	*selection = selection.iter().map(|key| keys[key].clone()).collect();
	// Existing semantic role bindings continue to identify the originals.
	data.entities.extend(copies);
	Ok(())
}

fn reparent(
	data: &mut SceneData,
	key: &str,
	parent: Option<&str>,
	keep_world: bool,
) -> Result<(), String> {
	let index = entity_index(data, key)?;
	let parent_index = parent.map(|key| entity_index(data, key)).transpose()?;
	if data.entities[index].parent.as_ref().map(EntityKey::as_str) == parent {
		return Ok(());
	}
	let hierarchy = hierarchy(data)?;
	let mut ancestor = parent_index;
	let mut remaining = data.entities.len();
	while let Some(parent) = ancestor {
		if parent == index {
			return Err("An entity cannot be parented to itself or one of its descendants".into());
		}
		if remaining == 0 {
			return Err("Scene hierarchy contains a parent cycle".into());
		}
		remaining -= 1;
		ancestor = hierarchy.parents[parent];
	}
	let transform = if keep_world {
		let world = data.world_matrices()?;
		let local = match parent_index {
			Some(parent) => inverse(world[parent])? * world[index],
			None => world[index],
		};
		Some(TransformData::from_matrix(local)?)
	} else {
		None
	};
	data.entities[index].parent = parent_index.map(|parent| data.entities[parent].key.clone());
	if let Some(transform) = transform {
		data.entities[index].transform = transform;
	}
	Ok(())
}

fn selection_bounds(
	data: &SceneData,
	selection: &[EntityKey],
	bounds: &HashMap<(String, usize), Bounds3>,
) -> Result<Bounds3, String> {
	let hierarchy = hierarchy(data)?;
	let roots = selection_roots(&hierarchy, selection)?;
	let indices = selected_subtree(&hierarchy, &roots);
	let world = data.world_matrices()?;
	let mut combined: Option<Bounds3> = None;
	for &index in &indices {
		let entity = &data.entities[index];
		// Imported transform-only nodes carry the source path as provenance,
		// not an implicit reference to mesh zero. Frame their descendants.
		let model_bounds = entity
			.renderable
			.then(|| {
				bounds
					.get(&(entity.model_path.clone(), entity.mesh_index.unwrap_or(0)))
					.copied()
			})
			.flatten();
		let local = match model_bounds {
			Some(bounds) => bounds,
			None if entity.collider.is_some() => {
				collider_bounds(entity.collider.as_ref()).expect("collider is present")?
			}
			None if !entity.renderable => continue,
			None => {
				return Err(format!(
					"Model bounds for {:?} mesh {} are unavailable; load the model in the viewport or add a collider before framing this selection",
					entity.model_path,
					entity.mesh_index.unwrap_or(0)
				));
			}
		};
		let transformed = local.transformed(world[index])?;
		combined = Some(combined.map_or(transformed, |bounds| bounds.union(transformed)));
	}
	// An empty transform group still has a useful pivot. Only use origins when
	// the selected subtree contains no renderable geometry or collider bounds.
	if combined.is_none() {
		for index in indices {
			let point = world[index].w_axis.truncate().to_array();
			let origin = Bounds3::new(point, point)?;
			combined = Some(combined.map_or(origin, |bounds| bounds.union(origin)));
		}
	}
	combined
		.ok_or_else(|| "Select at least one entity before using camera selection controls".into())
}

fn collider_bounds(collider: Option<&ColliderData>) -> Option<Result<Bounds3, String>> {
	collider.map(|collider| {
		let (half, offset) = match collider {
			ColliderData::Box {
				half_extents,
				offset,
			} => (Vec3::from_array(*half_extents), Vec3::from_array(*offset)),
			ColliderData::Sphere { radius, offset } => {
				(Vec3::splat(*radius), Vec3::from_array(*offset))
			}
		};
		Bounds3::new((offset - half).to_array(), (offset + half).to_array())
	})
}

fn bounds_center(bounds: Bounds3) -> Result<Vec3, String> {
	finite_result(
		Vec3::from_array(bounds.min()) * 0.5 + Vec3::from_array(bounds.max()) * 0.5,
		"Selection bounds center",
	)
}

fn camera_forward(camera: &CameraData) -> Vec3 {
	Vec3::new(
		camera.yaw.sin() * camera.pitch.cos(),
		camera.pitch.sin(),
		-camera.yaw.cos() * camera.pitch.cos(),
	)
}

fn camera_right(camera: &CameraData) -> Vec3 {
	Vec3::new(camera.yaw.cos(), 0.0, camera.yaw.sin())
}

fn camera_pivot(
	data: &SceneData,
	selection: &[EntityKey],
	bounds: &HashMap<(String, usize), Bounds3>,
) -> Result<Vec3, String> {
	if selection.is_empty() {
		finite_result(
			Vec3::from_array(data.camera.position) + camera_forward(&data.camera) * 3.0,
			"Camera pivot",
		)
	} else {
		bounds_center(selection_bounds(data, selection, bounds)?)
	}
}

fn frame_selection(
	data: &mut SceneData,
	selection: &[EntityKey],
	bounds: &HashMap<(String, usize), Bounds3>,
) -> Result<(), String> {
	let bounds = selection_bounds(data, selection, bounds)?;
	let center = bounds_center(bounds)?;
	let radius = (Vec3::from_array(bounds.max()) * 0.5 - Vec3::from_array(bounds.min()) * 0.5)
		.length()
		.max(0.01);
	let distance = (radius / (data.camera.vertical_fov * 0.5).sin() * 1.2)
		.max(radius + data.camera.near_plane * 2.0);
	finite_scalar(distance, "Camera framing distance")?;
	let position = center - camera_forward(&data.camera) * distance;
	data.camera.position = finite_result(position, "Camera framing position")?.to_array();
	data.camera.far_plane = data.camera.far_plane.max((distance + radius) * 1.2);
	Ok(())
}

fn orbit(
	data: &mut SceneData,
	selection: &[EntityKey],
	bounds: &HashMap<(String, usize), Bounds3>,
	yaw: f32,
	pitch: f32,
) -> Result<(), String> {
	finite_scalar(yaw, "Orbit yaw")?;
	finite_scalar(pitch, "Orbit pitch")?;
	if yaw == 0.0 && pitch == 0.0 {
		return Ok(());
	}
	let pivot = camera_pivot(data, selection, bounds)?;
	let distance = (Vec3::from_array(data.camera.position) - pivot)
		.length()
		.max(data.camera.near_plane * 2.0);
	finite_scalar(distance, "Orbit distance")?;
	// Reduce delta before addition to avoid overflowing two individually finite angles.
	data.camera.yaw = (data.camera.yaw.rem_euclid(std::f32::consts::TAU)
		+ yaw.rem_euclid(std::f32::consts::TAU))
	.rem_euclid(std::f32::consts::TAU);
	let limit = 89.0_f32.to_radians();
	data.camera.pitch =
		((data.camera.pitch as f64 + pitch as f64).clamp(-limit as f64, limit as f64)) as f32;
	data.camera.position = finite_result(
		pivot - camera_forward(&data.camera) * distance,
		"Orbit position",
	)?
	.to_array();
	Ok(())
}

fn dolly(
	data: &mut SceneData,
	selection: &[EntityKey],
	bounds: &HashMap<(String, usize), Bounds3>,
	distance: f32,
) -> Result<(), String> {
	finite_scalar(distance, "Dolly distance")?;
	if distance == 0.0 {
		return Ok(());
	}
	let pivot = camera_pivot(data, selection, bounds)?;
	let offset = Vec3::from_array(data.camera.position) - pivot;
	let old_distance = offset.length();
	finite_scalar(old_distance, "Dolly target distance")?;
	let direction = offset
		.try_normalize()
		.unwrap_or(-camera_forward(&data.camera));
	let distance =
		((old_distance as f64 - distance as f64).max(data.camera.near_plane as f64 * 2.0)) as f32;
	finite_scalar(distance, "Dolly result distance")?;
	data.camera.position =
		finite_result(pivot + direction * distance, "Dolly position")?.to_array();
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn selection_operations_preserve_hierarchy_error_order_before_mutation_or_noop() {
		let camera = crate::Camera::default();
		let key = |value: &str| EntityKey::new(value).unwrap();
		let original = SceneData {
			schema_version: crate::scene_data::CURRENT_SCHEMA_VERSION,
			bindings: Default::default(),
			entities: vec![
				EntityData::new(key("a"), "A", "model.glb"),
				EntityData::new(key("b"), "B", "model.glb"),
			],
			camera: CameraData {
				position: camera.position,
				yaw: camera.yaw,
				pitch: camera.pitch,
				vertical_fov: camera.vertical_fov,
				near_plane: camera.near,
				far_plane: camera.far,
			},
			lights: vec![],
		};
		let mut duplicate = original.clone();
		duplicate.entities[1].key = key("a");
		duplicate.entities[0].parent = Some(key("missing-parent"));
		let mut missing_parent = original.clone();
		missing_parent.entities[0].parent = Some(key("missing-parent"));
		let mut cycle = original.clone();
		cycle.entities[0].parent = Some(key("b"));
		cycle.entities[1].parent = Some(key("a"));
		for (data, selected, expected) in [
			(
				duplicate,
				"missing-selection",
				"Scene contains duplicate entity keys",
			),
			(
				missing_parent,
				"missing-selection",
				"Parent \"missing-parent\" does not exist",
			),
			(
				cycle.clone(),
				"missing-selection",
				"Selected entity \"missing-selection\" no longer exists",
			),
			(cycle, "a", "Scene hierarchy contains a parent cycle"),
		] {
			for command in [
				EditorCommandDto::DuplicateSelection { offset: [0.0; 3] },
				EditorCommandDto::DeleteSelection,
				EditorCommandDto::TranslateSelection {
					delta: [0.0; 3],
					snap: false,
				},
			] {
				let mut candidate = data.clone();
				let mut selection = vec![key(selected)];
				let error = apply(
					&mut candidate,
					&mut selection,
					&SnapSettingsDto::default(),
					&HashMap::new(),
					command,
				)
				.unwrap_err();
				assert_eq!(error, expected);
				assert_eq!(candidate, data);
				assert_eq!(selection, vec![key(selected)]);
			}
		}
	}
}
