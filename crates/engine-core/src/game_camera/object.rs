use super::{FollowCameraSettings, OrbitCameraSettings};
use crate::{
	Camera, EntityKey,
	components::{ComponentData, ComponentId, ComponentRecord, NO_MESH_PATH},
	scene_data::{CameraData, EntityData, SceneData, TransformData},
};
use glam::{Mat4, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerspectiveLens {
	pub vertical_fov: f32,
	#[serde(rename = "near")]
	pub near_plane: f32,
	#[serde(rename = "far")]
	pub far_plane: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
	tag = "mode",
	content = "settings",
	rename_all = "snake_case",
	deny_unknown_fields
)]
pub enum CameraRig {
	Fixed,
	Follow(FollowCameraSettings),
	Orbit(OrbitCameraSettings),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraSettings {
	pub lens: PerspectiveLens,
	pub rig: CameraRig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraComponent {
	pub id: ComponentId,
	pub enabled: bool,
	pub settings: CameraSettings,
}

impl CameraRig {
	pub fn target(&self) -> Option<&EntityKey> {
		match self {
			Self::Fixed => None,
			Self::Follow(settings) => Some(&settings.target),
			Self::Orbit(settings) => Some(&settings.target),
		}
	}

	pub(crate) fn remap_target(&mut self, remap: &HashMap<EntityKey, EntityKey>) {
		let target = match self {
			Self::Fixed => return,
			Self::Follow(settings) => &mut settings.target,
			Self::Orbit(settings) => &mut settings.target,
		};
		if let Some(replacement) = remap.get(target) {
			target.clone_from(replacement);
		}
	}
}

impl CameraSettings {
	pub fn validate(&self) -> Result<(), String> {
		self.project([0.0; 3], 0.0, 0.0).validate()
	}

	pub fn from_legacy(camera: &CameraData) -> Self {
		Self {
			lens: PerspectiveLens {
				vertical_fov: camera.vertical_fov,
				near_plane: camera.near_plane,
				far_plane: camera.far_plane,
			},
			rig: if let Some(follow) = &camera.follow {
				CameraRig::Follow(follow.clone())
			} else if let Some(orbit) = &camera.orbit {
				CameraRig::Orbit(orbit.clone())
			} else {
				CameraRig::Fixed
			},
		}
	}

	/// Scene-space camera axes use local -Z forward and +Y up. The lens does
	/// not inherit scale. The current camera boundary uses world-up yaw/pitch.
	pub fn from_world_matrix(&self, world: Mat4) -> Result<CameraData, String> {
		if !world.is_finite() || world.row(3) != glam::Vec4::W {
			return Err("Camera world transform must be finite and affine".into());
		}
		let world_right = normalized_axis(world.x_axis.truncate())?;
		let world_up = normalized_axis(world.y_axis.truncate())?;
		let forward = normalized_axis(-world.z_axis.truncate())?;
		let horizontal = forward.x.hypot(forward.z);
		let yaw = forward.x.atan2(-forward.z);
		let pitch = forward.y.atan2(horizontal);
		let pitch_limit = 89.0_f32.to_radians();
		let world_up_right = Vec3::new(yaw.cos(), 0.0, yaw.sin());
		const AXIS_TOLERANCE: f32 = 1.0e-4;
		if pitch.abs() > pitch_limit + AXIS_TOLERANCE
			|| !world_right.abs_diff_eq(world_up_right, AXIS_TOLERANCE)
			|| !world_up.abs_diff_eq(world_up_right.cross(forward), AXIS_TOLERANCE)
		{
			return Err("Camera world pose requires an upright basis without roll or shear and pitch within ±89 degrees".into());
		}
		let projected = self.project(
			world.w_axis.truncate().to_array(),
			yaw,
			pitch.clamp(-pitch_limit, pitch_limit),
		);
		projected.validate()?;
		Ok(projected)
	}

	fn project(&self, position: [f32; 3], yaw: f32, pitch: f32) -> CameraData {
		let (follow, orbit) = match &self.rig {
			CameraRig::Fixed => (None, None),
			CameraRig::Follow(settings) => (Some(settings.clone()), None),
			CameraRig::Orbit(settings) => (None, Some(settings.clone())),
		};
		CameraData {
			follow,
			orbit,
			position,
			yaw,
			pitch,
			vertical_fov: self.lens.vertical_fov,
			near_plane: self.lens.near_plane,
			far_plane: self.lens.far_plane,
		}
	}
}

impl CameraData {
	pub fn to_camera(&self) -> Camera {
		Camera {
			position: self.position,
			yaw: self.yaw,
			pitch: self.pitch,
			vertical_fov: self.vertical_fov,
			near: self.near_plane,
			far: self.far_plane,
		}
	}

	pub(crate) fn pose_matrix(&self) -> Mat4 {
		let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
		let (sin_pitch, cos_pitch) = self.pitch.sin_cos();
		let right = Vec3::new(cos_yaw, 0.0, sin_yaw);
		let forward = Vec3::new(sin_yaw * cos_pitch, sin_pitch, -cos_yaw * cos_pitch);
		Mat4::from_cols(
			right.extend(0.0),
			right.cross(forward).extend(0.0),
			(-forward).extend(0.0),
			Vec3::from_array(self.position).extend(1.0),
		)
	}

	pub(crate) fn to_camera_object(
		&self,
		key: EntityKey,
		name: &str,
	) -> Result<EntityData, String> {
		let mut object = EntityData::new(key, name, NO_MESH_PATH)
			.with_renderable(false)
			.with_transform(TransformData::from_matrix(self.pose_matrix())?);
		object.put_component(
			ComponentRecord {
				id: ComponentId::new("camera")?,
				schema_version: 1,
				enabled: true,
				data: ComponentData::Camera {
					settings: CameraSettings::from_legacy(self),
				},
			},
			false,
		)?;
		Ok(object)
	}
}

impl SceneData {
	pub fn authored_game_camera(&self) -> Result<CameraData, String> {
		match &self.default_game_camera {
			Some(key) => self.object_camera(key, &self.world_matrices()?),
			None => Ok(self.camera.clone()),
		}
	}

	pub fn object_camera(&self, key: &EntityKey, world: &[Mat4]) -> Result<CameraData, String> {
		let index = self.camera_index(key)?;
		let settings = &self.entities[index]
			.camera
			.as_ref()
			.expect("checked camera")
			.settings;
		let matrix = *world
			.get(index)
			.ok_or("Camera world transform is missing")?;
		settings.from_world_matrix(matrix)
	}

	pub(crate) fn camera_index(&self, key: &EntityKey) -> Result<usize, String> {
		self.entities
			.iter()
			.position(|entity| {
				&entity.key == key && entity.camera.as_ref().is_some_and(|c| c.enabled)
			})
			.ok_or_else(|| {
				format!(
					"Game camera {:?} must refer to an enabled Camera object in this scene",
					key.as_str()
				)
			})
	}

	pub(crate) fn refresh_game_camera_projection(&mut self) -> Result<(), String> {
		if self.default_game_camera.is_some() {
			self.camera = self.authored_game_camera()?;
		}
		Ok(())
	}

	pub(crate) fn set_authored_game_camera(&mut self, camera: CameraData) -> Result<(), String> {
		camera.validate()?;
		if let Some(key) = &self.default_game_camera {
			let index = self.camera_index(key)?;
			if self.camera.position != camera.position
				|| self.camera.yaw != camera.yaw
				|| self.camera.pitch != camera.pitch
			{
				self.set_camera_object_pose(index, &camera)?;
			}
			let entity = &mut self.entities[index];
			entity.camera.as_mut().expect("checked camera").settings =
				CameraSettings::from_legacy(&camera);
		}
		self.camera = camera;
		Ok(())
	}

	pub(crate) fn set_camera_object_pose(
		&mut self,
		index: usize,
		camera: &CameraData,
	) -> Result<(), String> {
		let entity = &self.entities[index];
		let world_pose = camera.pose_matrix();
		let local = if let Some(parent) = &entity.parent {
			let world = self.world_matrices()?;
			let parent_index = self
				.entities
				.iter()
				.position(|entity| &entity.key == parent)
				.ok_or("Camera parent is missing")?;
			world[parent_index].inverse() * world_pose
		} else {
			world_pose
		};
		let mut transform = TransformData::from_matrix(local)?;
		transform.scale = entity.transform.scale;
		self.entities[index].transform = transform;
		Ok(())
	}

	pub(crate) fn remove_camera_targets(&mut self, removed: &HashSet<EntityKey>) {
		self.camera.clear_removed_targets(removed);
		if self
			.default_game_camera
			.as_ref()
			.is_some_and(|key| removed.contains(key))
		{
			self.default_game_camera = None;
		}
		for entity in &mut self.entities {
			if let Some(camera) = &mut entity.camera
				&& camera
					.settings
					.rig
					.target()
					.is_some_and(|key| removed.contains(key))
			{
				camera.settings.rig = CameraRig::Fixed;
			}
		}
	}

	pub(crate) fn legacy_camera_object(&self) -> Result<EntityData, String> {
		let occupied: HashSet<_> = self
			.entities
			.iter()
			.map(|entity| entity.key.as_str())
			.chain(self.lights.iter().map(|light| light.key.as_str()))
			.collect();
		for suffix in 0..=occupied.len() {
			let candidate = if suffix == 0 {
				"legacy-game-camera".into()
			} else {
				format!("legacy-game-camera-{suffix}")
			};
			if !occupied.contains(candidate.as_str()) {
				return self
					.camera
					.to_camera_object(EntityKey::new(candidate)?, "Game Camera");
			}
		}
		Err("Could not allocate a stable migrated Game Camera key".into())
	}
}

fn normalized_axis(axis: Vec3) -> Result<Vec3, String> {
	axis.try_normalize()
		.ok_or_else(|| "Camera world transform must have finite nonzero axes".into())
}
