use super::*;

pub(super) fn create_camera(data: &mut SceneData) -> Result<EntityKey, String> {
	if data.entities.len() >= MAX_SCENE_ENTITIES {
		return Err("Scene entity limit reached; remove entities before creating a Camera".into());
	}
	let key = fresh_key(data);
	let default = if data.default_game_camera.is_none() {
		data.camera.clone()
	} else {
		CameraData::default()
	};
	data.entities
		.push(default.to_camera_object(key.clone(), "Camera")?);
	if data.default_game_camera.is_none() {
		data.default_game_camera = Some(key.clone());
	}
	Ok(key)
}

pub(super) fn align_camera(
	data: &mut SceneData,
	key: &EntityKey,
	position: [f32; 3],
	yaw: f32,
	pitch: f32,
) -> Result<(), String> {
	let index = data.camera_index(key)?;
	let camera = CameraData {
		position,
		yaw,
		pitch,
		..CameraData::default()
	};
	data.set_camera_object_pose(index, &camera)
}

fn selection_bounds(
	data: &SceneData,
	selection: &[EntityKey],
	bounds: &ModelBounds,
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
			.then(|| bounds.get(entity).copied())
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
				..
			} => (Vec3::from_array(*half_extents), Vec3::from_array(*offset)),
			ColliderData::Sphere { radius, offset, .. } => {
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

pub(super) fn camera_forward(camera: &CameraData) -> Vec3 {
	Vec3::new(
		camera.yaw.sin() * camera.pitch.cos(),
		camera.pitch.sin(),
		-camera.yaw.cos() * camera.pitch.cos(),
	)
}

pub(super) fn camera_right(camera: &CameraData) -> Vec3 {
	Vec3::new(camera.yaw.cos(), 0.0, camera.yaw.sin())
}

fn camera_pivot(
	data: &SceneData,
	selection: &[EntityKey],
	bounds: &ModelBounds,
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

pub(super) fn frame_selection(
	data: &mut SceneData,
	selection: &[EntityKey],
	bounds: &ModelBounds,
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
	let mut camera = data.camera.clone();
	camera.position = finite_result(position, "Camera framing position")?.to_array();
	camera.far_plane = camera.far_plane.max((distance + radius) * 1.2);
	data.set_authored_game_camera(camera)
}

pub(super) fn orbit(
	data: &mut SceneData,
	selection: &[EntityKey],
	bounds: &ModelBounds,
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
	let mut camera = data.camera.clone();
	camera.yaw = (camera.yaw.rem_euclid(std::f32::consts::TAU)
		+ yaw.rem_euclid(std::f32::consts::TAU))
	.rem_euclid(std::f32::consts::TAU);
	let limit = 89.0_f32.to_radians();
	camera.pitch = ((camera.pitch as f64 + pitch as f64).clamp(-limit as f64, limit as f64)) as f32;
	camera.position =
		finite_result(pivot - camera_forward(&camera) * distance, "Orbit position")?.to_array();
	data.set_authored_game_camera(camera)
}

pub(super) fn dolly(
	data: &mut SceneData,
	selection: &[EntityKey],
	bounds: &ModelBounds,
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
	let mut camera = data.camera.clone();
	camera.position = finite_result(pivot + direction * distance, "Dolly position")?.to_array();
	data.set_authored_game_camera(camera)
}
