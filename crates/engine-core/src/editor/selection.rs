use super::*;

impl EditorSession {
	pub(super) fn pick_selection(
		&mut self,
		pixel: [f64; 2],
		extent: [u32; 2],
		additive: bool,
	) -> Result<(), String> {
		self.require_edit()?;
		let data = self.data.as_ref();
		let camera = crate::Camera {
			position: data.camera.position,
			yaw: data.camera.yaw,
			pitch: data.camera.pitch,
			vertical_fov: data.camera.vertical_fov,
			near: data.camera.near_plane,
			far: data.camera.far_plane,
		};
		let ray = crate::picking::CameraRay::from_screen(&camera, pixel, extent)?;
		let world = data.world_matrices()?;
		let mut nearest = ray.max_distance;
		let mut selected = None;
		for (entity, matrix) in data.entities.iter().zip(world) {
			if !entity.renderable {
				continue;
			}
			let Some(bounds) = self.bounds.get(entity) else {
				continue;
			};
			if !matrix.is_finite() || matrix.determinant() == 0.0 {
				continue;
			}
			let inverse = matrix.inverse();
			if let Some(distance) = bounds.ray_distance(
				inverse.transform_point3(ray.origin),
				inverse.transform_vector3(ray.direction),
				nearest,
			) && (selected.is_none() || distance < nearest)
			{
				nearest = distance;
				selected = Some(entity.key.clone());
			}
		}
		let mut selection = if additive {
			self.selection.clone()
		} else {
			Vec::new()
		};
		if let Some(key) = selected
			&& !selection.contains(&key)
		{
			if selection.len() >= MAX_SELECTION {
				return Err("Selection exceeds 4096 entities".into());
			}
			selection.push(key);
		}
		if selection != self.selection {
			self.selection = selection;
			self.bump();
		}
		Ok(())
	}

	pub(super) fn select_keys(&mut self, keys: Vec<String>) -> Result<(), String> {
		if keys.len() > MAX_SELECTION {
			return Err("Selection exceeds 4096 entities".into());
		}
		let mut selection = Vec::with_capacity(keys.len());
		let document = self
			.play
			.as_ref()
			.map_or(self.data.as_ref(), |play| play.active().world().data());
		let available: HashSet<_> = document
			.entities
			.iter()
			.map(|entity| entity.key.as_str())
			.collect();
		let mut unique = HashSet::with_capacity(keys.len());
		for key in keys {
			let key = EntityKey::new(key)?;
			if !available.contains(key.as_str()) {
				return Err("Selection contains a missing entity".into());
			}
			if unique.insert(key.clone()) {
				selection.push(key);
			}
		}
		if selection != self.selection {
			self.selection = selection;
			self.bump();
		}
		Ok(())
	}
}

pub(super) fn validate_selection(data: &SceneData, selection: &[EntityKey]) -> Result<(), String> {
	if selection.len() > MAX_SELECTION {
		return Err("Selection exceeds 4096 entities".into());
	}
	let available: HashSet<_> = data.entities.iter().map(|entity| &entity.key).collect();
	let mut unique = HashSet::with_capacity(selection.len());
	for key in selection {
		if !available.contains(key) || !unique.insert(key) {
			return Err("Selection must contain distinct existing entities".into());
		}
	}
	Ok(())
}
