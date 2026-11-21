use super::*;

pub(super) fn entity_index(data: &SceneData, key: &str) -> Result<usize, String> {
	data.entities
		.iter()
		.position(|entity| entity.key.as_str() == key)
		.ok_or_else(|| format!("Entity key {key:?} does not exist"))
}

pub(super) fn entity_indices(data: &SceneData) -> HashMap<&str, usize> {
	let mut indices = HashMap::with_capacity(data.entities.len());
	for (index, entity) in data.entities.iter().enumerate() {
		// Match the single-entity scan even before scene validation rejects duplicates.
		indices.entry(entity.key.as_str()).or_insert(index);
	}
	indices
}

pub(super) fn finite_scalar(value: f32, label: &str) -> Result<(), String> {
	if value.is_finite() {
		Ok(())
	} else {
		Err(format!("{label} must be finite"))
	}
}

pub(super) fn finite_vec(value: [f32; 3], label: &str) -> Result<Vec3, String> {
	finite_result(Vec3::from_array(value), label)
}

pub(super) fn finite_result(value: Vec3, label: &str) -> Result<Vec3, String> {
	if value.is_finite() {
		Ok(value)
	} else {
		Err(format!(
			"{label} must remain within the finite supported range"
		))
	}
}

pub(super) fn transform_from_dto(value: TransformDto) -> Result<TransformData, String> {
	finite_vec(value.position, "Position")?;
	finite_vec(value.rotation, "Rotation")?;
	finite_vec(value.scale, "Scale")?;
	Ok(TransformData {
		position: value.position,
		rotation: value.rotation,
		scale: value.scale,
	})
}

pub(super) fn snapping(settings: &SnapSettingsDto, command_enabled: bool) -> Result<bool, String> {
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

pub(super) fn snap_vec(value: Vec3, increment: f32) -> Vec3 {
	// Divide in f64 so a finite f32 value and small grid increment do not
	// overflow merely while computing the nearest grid coordinate.
	Vec3::from_array(
		value
			.to_array()
			.map(|v| ((v as f64 / increment as f64).round() * increment as f64) as f32),
	)
}

pub(super) fn snap_transform(transform: &mut TransformData, settings: &SnapSettingsDto) {
	transform.position =
		snap_vec(Vec3::from_array(transform.position), settings.translation).to_array();
	transform.rotation = snap_vec(
		Vec3::from_array(transform.rotation),
		settings.rotation_radians,
	)
	.to_array();
	transform.scale = snap_vec(Vec3::from_array(transform.scale), settings.scale).to_array();
}

pub(super) struct Hierarchy<'a> {
	pub(super) indices: HashMap<&'a str, usize>,
	pub(super) parents: Vec<Option<usize>>,
}

pub(super) fn hierarchy(data: &SceneData) -> Result<Hierarchy<'_>, String> {
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

pub(super) fn selection_roots(
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

pub(super) fn selected_subtree(hierarchy: &Hierarchy<'_>, roots: &[usize]) -> Vec<usize> {
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
pub(super) fn set_world_targets(
	data: &mut SceneData,
	targets: HashMap<usize, Mat4>,
) -> Result<(), String> {
	let original = data.world_matrices()?;
	let locals = world_target_locals(data, targets, &original, None)?;
	for (index, transform) in locals {
		data.entities[index].transform = transform;
	}
	Ok(())
}

pub(super) fn world_target_locals(
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

pub(super) fn inverse(matrix: Mat4) -> Result<Mat4, String> {
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

pub(super) fn root_pivot(roots: &[usize], world: &[Mat4]) -> Result<Vec3, String> {
	let count = roots.len() as f32;
	finite_result(
		roots.iter().fold(Vec3::ZERO, |sum, &index| {
			sum + world[index].w_axis.truncate() / count
		}),
		"Selection pivot",
	)
}
