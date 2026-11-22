mod hierarchy;
mod organization;
pub use crate::collider::{ColliderData, ColliderInteraction};
pub use crate::game_camera::{FollowCameraSettings, OrbitCameraSettings};
pub use crate::scene_format::parse_json;
use crate::{Transform, entity_key::EntityKey};
use glam::{EulerRot, Mat4};
pub use hierarchy::resolve_world_matrices;
pub use organization::{
	FolderId, FolderRecord, MAX_FOLDER_DEPTH, MAX_FOLDER_ID_BYTES, MAX_FOLDER_NAME_BYTES,
	MAX_SCENE_FOLDERS, SceneOrganization,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

pub const CURRENT_SCHEMA_VERSION: u32 = 6;
pub const MAX_SCENE_ENTITIES: usize = 100_000;
pub const MAX_SCENE_LIGHTS: usize = 16;
pub const MAX_SCENE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SceneData {
	pub schema_version: u32,
	pub bindings: BTreeMap<String, EntityKey>,
	pub entities: Vec<EntityData>,
	pub camera: CameraData,
	#[serde(default)]
	pub lights: Vec<LightData>,
	#[serde(skip_serializing_if = "SceneOrganization::is_empty")]
	pub organization: SceneOrganization,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "EntityProjection")]
pub struct EntityData {
	pub key: EntityKey,
	pub name: String,
	pub model_path: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub asset_id: Option<crate::asset_catalog::AssetId>,
	pub transform: TransformData,
	#[serde(default = "default_renderable")]
	pub renderable: bool,
	/// Runtime actions mutate the Play copy; Stop/Restart restores this authored value.
	#[serde(default = "default_renderable")]
	pub collision_enabled: bool,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub parent: Option<EntityKey>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub material: Option<MaterialOverrideData>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub collider: Option<ColliderData>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub mesh_index: Option<usize>,
	#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
	pub component_ids: BTreeMap<String, crate::components::ComponentId>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub native_behaviors: Vec<crate::components::NativeBehaviorComponent>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub character_controller: Option<crate::character_controller::CharacterControllerComponent>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub animation: Option<crate::animation::AnimationComponent>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub interactable: Option<crate::components::InteractableComponent>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub linked_prefab: Option<crate::linked_prefab::LinkedPrefabInstance>,
}

fn default_renderable() -> bool {
	true
}

impl EntityData {
	/// Factory defaults are independent of the editor's keyboard or UI layout.
	pub fn new(key: EntityKey, name: impl Into<String>, model_path: impl Into<String>) -> Self {
		Self {
			key,
			name: name.into(),
			model_path: model_path.into(),
			asset_id: None,
			transform: TransformData::default(),
			renderable: true,
			collision_enabled: true,
			parent: None,
			material: None,
			collider: None,
			mesh_index: None,
			component_ids: BTreeMap::new(),
			native_behaviors: Vec::new(),
			character_controller: None,
			animation: None,
			interactable: None,
			linked_prefab: None,
		}
	}
	pub fn with_transform(mut self, transform: TransformData) -> Self {
		self.transform = transform;
		self
	}
	pub fn with_asset_id(mut self, asset_id: Option<crate::asset_catalog::AssetId>) -> Self {
		self.asset_id = asset_id;
		self
	}
	pub fn with_parent(mut self, parent: Option<EntityKey>) -> Self {
		self.parent = parent;
		self
	}
	pub fn with_material(mut self, material: Option<MaterialOverrideData>) -> Self {
		self.material = material;
		self
	}
	pub fn with_collider(mut self, collider: Option<ColliderData>) -> Self {
		self.collider = collider;
		self
	}
	pub fn with_mesh_index(mut self, mesh_index: Option<usize>) -> Self {
		self.mesh_index = mesh_index;
		self
	}
	pub fn with_renderable(mut self, renderable: bool) -> Self {
		self.renderable = renderable;
		self
	}
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntityProjection {
	key: EntityKey,
	name: String,
	model_path: String,
	transform: TransformData,
	#[serde(default)]
	asset_id: Option<crate::asset_catalog::AssetId>,
	#[serde(default = "default_renderable")]
	renderable: bool,
	#[serde(default = "default_renderable")]
	collision_enabled: bool,
	#[serde(default)]
	parent: Option<EntityKey>,
	#[serde(default)]
	material: Option<MaterialOverrideData>,
	#[serde(default)]
	collider: Option<ColliderData>,
	#[serde(default)]
	mesh_index: Option<usize>,
	#[serde(default)]
	component_ids: BTreeMap<String, crate::components::ComponentId>,
	#[serde(default)]
	native_behaviors: Vec<crate::components::NativeBehaviorComponent>,
	#[serde(default)]
	character_controller: Option<crate::character_controller::CharacterControllerComponent>,
	#[serde(default)]
	animation: Option<crate::animation::AnimationComponent>,
	#[serde(default)]
	interactable: Option<crate::components::InteractableComponent>,
	#[serde(default)]
	linked_prefab: Option<crate::linked_prefab::LinkedPrefabInstance>,
	/// A read-only transport projection; commands never take this as mutation authority.
	#[serde(default)]
	component_views: Option<Vec<crate::components::ComponentRecord>>,
}
impl TryFrom<EntityProjection> for EntityData {
	type Error = String;
	fn try_from(value: EntityProjection) -> Result<Self, String> {
		if let Some(views) = &value.component_views {
			if views.len() > crate::components::MAX_COMPONENTS_PER_OBJECT {
				return Err("Component projection exceeds its count limit".into());
			}
			for view in views {
				view.validate()?;
			}
		}
		Ok(Self {
			key: value.key,
			name: value.name,
			model_path: value.model_path,
			asset_id: value.asset_id,
			transform: value.transform,
			renderable: value.renderable,
			collision_enabled: value.collision_enabled,
			parent: value.parent,
			material: value.material,
			collider: value.collider,
			mesh_index: value.mesh_index,
			component_ids: value.component_ids,
			native_behaviors: value.native_behaviors,
			character_controller: value.character_controller,
			animation: value.animation,
			interactable: value.interactable,
			linked_prefab: value.linked_prefab,
		})
	}
}
impl Serialize for EntityData {
	fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		use serde::ser::SerializeMap;
		let mut map = serializer.serialize_map(None)?;
		map.serialize_entry("key", &self.key)?;
		map.serialize_entry("name", &self.name)?;
		map.serialize_entry("model_path", &self.model_path)?;
		map.serialize_entry("transform", &self.transform)?;
		map.serialize_entry("renderable", &self.renderable)?;
		map.serialize_entry("collision_enabled", &self.collision_enabled)?;
		if let Some(value) = &self.asset_id {
			map.serialize_entry("asset_id", value)?;
		}
		if let Some(value) = &self.parent {
			map.serialize_entry("parent", value)?;
		}
		if let Some(value) = &self.material {
			map.serialize_entry("material", value)?;
		}
		if let Some(value) = &self.collider {
			map.serialize_entry("collider", value)?;
		}
		if let Some(value) = &self.mesh_index {
			map.serialize_entry("mesh_index", value)?;
		}
		if !self.component_ids.is_empty() {
			map.serialize_entry("component_ids", &self.component_ids)?;
		}
		if !self.native_behaviors.is_empty() {
			map.serialize_entry("native_behaviors", &self.native_behaviors)?;
		}
		if let Some(value) = &self.character_controller {
			map.serialize_entry("character_controller", value)?;
		}
		if let Some(value) = &self.animation {
			map.serialize_entry("animation", value)?;
		}
		if let Some(value) = &self.interactable {
			map.serialize_entry("interactable", value)?;
		}
		if let Some(value) = &self.linked_prefab {
			map.serialize_entry("linked_prefab", value)?;
		}
		map.serialize_entry("component_views", &self.component_records())?;
		map.end()
	}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialOverrideData {
	pub base_color_factor: [f32; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LightKind {
	Directional,
	Point,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightData {
	pub key: EntityKey,
	pub name: String,
	pub kind: LightKind,
	pub position: [f32; 3],
	pub direction: [f32; 3],
	pub color: [f32; 3],
	pub intensity: f32,
	pub range: f32,
}

impl LightData {
	pub fn new(key: EntityKey, name: impl Into<String>, kind: LightKind) -> Self {
		Self {
			key,
			name: name.into(),
			kind,
			position: [0.0; 3],
			direction: [0.4, -0.8, -0.6],
			color: [1.0; 3],
			intensity: 0.75,
			range: 10.0,
		}
	}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransformData {
	pub position: [f32; 3],
	pub rotation: [f32; 3],
	pub scale: [f32; 3],
}

impl Default for TransformData {
	fn default() -> Self {
		Self {
			position: [0.0; 3],
			rotation: [0.0; 3],
			scale: [1.0; 3],
		}
	}
}

impl TransformData {
	pub fn matrix(&self) -> Mat4 {
		Transform {
			position: self.position,
			rotation: self.rotation,
			scale: self.scale,
		}
		.matrix()
	}
	/// A reparent operation may introduce shear. Never silently lose it by decomposing TRS.
	pub fn from_matrix(matrix: Mat4) -> Result<Self, String> {
		if !matrix.is_finite() || matrix.determinant() == 0.0 {
			return Err("Transform matrix must be finite and nonsingular".into());
		}
		let (scale, rotation, position) = matrix.to_scale_rotation_translation();
		let (x, y, z) = rotation.to_euler(EulerRot::XYZ);
		let value = Self {
			position: position.to_array(),
			rotation: [x, y, z],
			scale: scale.to_array(),
		};
		let rebuilt = value.matrix().to_cols_array();
		if !rebuilt.iter().all(|v| v.is_finite())
			|| rebuilt
				.iter()
				.zip(matrix.to_cols_array())
				.any(|(a, b)| (a - b).abs() > 1.0e-4 * b.abs().max(1.0))
		{
			return Err("Transform contains shear or perspective that cannot be represented as position/rotation/scale".into());
		}
		Ok(value)
	}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraData {
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub follow: Option<crate::game_camera::FollowCameraSettings>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub orbit: Option<crate::game_camera::OrbitCameraSettings>,
	pub position: [f32; 3],
	pub yaw: f32,
	pub pitch: f32,
	pub vertical_fov: f32,
	#[serde(rename = "near")]
	pub near_plane: f32,
	#[serde(rename = "far")]
	pub far_plane: f32,
}
#[derive(Debug)]
pub struct ParsedScene {
	pub data: SceneData,
	pub source_schema_version: u32,
}

impl SceneData {
	pub fn world_matrices(&self) -> Result<Vec<Mat4>, String> {
		let local: Vec<_> = self
			.entities
			.iter()
			.map(|entry| entry.transform.matrix())
			.collect();
		let keys: Vec<_> = self.entities.iter().map(|entry| Some(&entry.key)).collect();
		let parents: Vec<_> = self
			.entities
			.iter()
			.map(|entry| entry.parent.as_ref())
			.collect();
		resolve_world_matrices(&local, &keys, &parents)
	}

	pub fn validate(&self) -> Result<(), String> {
		self.validated_world_matrices().map(|_| ())
	}

	/// Validate the document and retain the world-transform work for callers
	/// that immediately synchronize a render scene.
	pub fn validated_world_matrices(&self) -> Result<Vec<Mat4>, String> {
		if self.schema_version != CURRENT_SCHEMA_VERSION {
			return Err(format!(
				"Unsupported schema version: {}",
				self.schema_version
			));
		}
		if self.entities.len() > MAX_SCENE_ENTITIES {
			return Err(format!(
				"A scene supports at most {MAX_SCENE_ENTITIES} entities"
			));
		}
		if self.lights.len() > MAX_SCENE_LIGHTS {
			return Err(format!(
				"A scene supports at most {MAX_SCENE_LIGHTS} lights"
			));
		}
		if self.bindings.len() > MAX_SCENE_ENTITIES {
			return Err("Scene has too many named bindings".into());
		}
		// Bound direct typed authoring as well as parsed files. The fixed charge
		// conservatively covers pretty JSON field names, indentation and every
		// optional numeric component; strings include JSON escaping. This avoids
		// serializing a whole scene after every command just to enforce a budget.
		let mut authored_bytes = 4096usize;
		let mut charge = |fixed: usize, strings: &[&str]| -> Result<(), String> {
			authored_bytes = authored_bytes.saturating_add(fixed);
			for value in strings {
				authored_bytes = authored_bytes.saturating_add(2);
				for byte in value.bytes() {
					authored_bytes = authored_bytes.saturating_add(match byte {
						b'"' | b'\\' => 2,
						0..=31 => 6,
						_ => 1,
					});
				}
			}
			if authored_bytes >= MAX_SCENE_BYTES {
				return Err("Scene authored-data budget exceeds 32 MiB".into());
			}
			Ok(())
		};
		let mut keys = HashSet::new();
		for (index, entity) in self.entities.iter().enumerate() {
			charge(
				2048,
				&[
					entity.key.as_str(),
					&entity.name,
					&entity.model_path,
					entity.parent.as_ref().map_or("", |key| key.as_str()),
				],
			)?;
			let context = format!("entity {index} ({:?})", entity.name);
			entity
				.key
				.validate()
				.map_err(|error| format!("{context}: {error}"))?;
			if entity.key.as_str().len() > 256
				|| entity.key.as_str().chars().any(char::is_control)
				|| !keys.insert(entity.key.as_str())
			{
				return Err(format!("{context}: duplicate or oversized entity key"));
			}
			validate_name(&entity.name).map_err(|error| format!("{context}: {error}"))?;
			validate_model_path(&entity.model_path)
				.map_err(|error| format!("{context}: {error}"))?;
			if !entity
				.transform
				.position
				.iter()
				.chain(entity.transform.rotation.iter())
				.chain(entity.transform.scale.iter())
				.all(|v| v.is_finite())
			{
				return Err(format!("{context}: transform values must all be finite"));
			}
			if let Some(material) = &entity.material
				&& !material
					.base_color_factor
					.iter()
					.all(|v| v.is_finite() && (0.0..=1.0).contains(v))
			{
				return Err(format!("{context}: material color must be within 0..=1"));
			}
			if let Some(collider) = &entity.collider {
				collider
					.validate()
					.map_err(|error| format!("{context}: {error}"))?;
			}
			if entity.mesh_index.is_some_and(|index| index > 1_000_000) {
				return Err(format!("{context}: mesh index exceeds supported limit"));
			}
		}
		if let Some(follow) = &self.camera.follow {
			charge(256, &[follow.target.as_str()])?;
			if !keys.contains(follow.target.as_str()) {
				return Err("Follow camera target must reference an entity in this scene".into());
			}
		}
		if let Some(orbit) = &self.camera.orbit {
			charge(256, &[orbit.target.as_str()])?;
			if !keys.contains(orbit.target.as_str()) {
				return Err("Orbit camera target must reference an entity in this scene".into());
			}
		}
		for (name, key) in &self.bindings {
			charge(32, &[name, key.as_str()])?;
			validate_name(name).map_err(|error| format!("Binding {name:?}: {error}"))?;
			if !keys.contains(key.as_str()) {
				return Err(format!(
					"Binding {name:?} targets missing entity key {:?}",
					key.as_str()
				));
			}
		}
		self.organization.validate(&self.entities)?;
		for folder in &self.organization.folders {
			charge(
				96,
				&[
					folder.id.as_str(),
					&folder.name,
					folder.parent.as_ref().map_or("", FolderId::as_str),
				],
			)?;
		}
		for (entity, folder) in &self.organization.root_membership {
			charge(24, &[entity.as_str(), folder.as_str()])?;
		}
		// Iterative traversal rejects cycles without recursive stack growth.
		let world = self.world_matrices()?;
		for light in &self.lights {
			charge(1024, &[light.key.as_str(), &light.name])?;
			light.key.validate()?;
			if light.key.as_str().len() > 256
				|| light.key.as_str().chars().any(char::is_control)
				|| !keys.insert(light.key.as_str())
			{
				return Err("Light keys must be unique and distinct from entity keys".into());
			}
			validate_name(&light.name)?;
			if !light
				.position
				.iter()
				.chain(light.direction.iter())
				.all(|v| v.is_finite())
				|| !light
					.color
					.iter()
					.all(|v| v.is_finite() && (0.0..=1.0).contains(v))
				|| !light.intensity.is_finite()
				|| light.intensity < 0.0
				|| light.intensity > 1_000_000.0
				|| !light.range.is_finite()
				|| light.range <= 0.0
			{
				return Err(format!(
					"Light {:?} contains invalid position, color, intensity or range",
					light.name
				));
			}
			if light.kind == LightKind::Directional
				&& (glam::Vec3::from_array(light.direction).length_squared() < 1.0e-12
					|| !glam::Vec3::from_array(light.direction)
						.length_squared()
						.is_finite())
			{
				return Err("Directional lights require a nonzero direction".into());
			}
		}
		crate::components::validate_scene_components(self)?;
		crate::linked_prefab::validate_scene_links(self)?;
		self.camera.validate()?;
		crate::scene_format::validate_authored_size(self)?;
		Ok(world)
	}

	/// Validate once while an immutable borrow keeps the checked content unchanged.
	pub fn validated(&self) -> Result<ValidatedScene<'_>, String> {
		self.validate()?;
		Ok(ValidatedScene::from_validated(self))
	}
}

/// An immutable view of scene data whose domain invariants have been checked.
#[derive(Clone, Copy)]
pub struct ValidatedScene<'a> {
	data: &'a SceneData,
}
impl<'a> ValidatedScene<'a> {
	pub fn data(self) -> &'a SceneData {
		self.data
	}
	pub(crate) fn from_validated(data: &'a SceneData) -> Self {
		Self { data }
	}
}

pub fn validate_model_path(path: &str) -> Result<(), String> {
	if path.trim().is_empty()
		|| path.len() > 4096
		|| path.chars().any(char::is_control)
		|| path.starts_with('/')
		|| path.contains('\\')
		|| path.contains(':')
		|| path
			.split('/')
			.any(|part| part == ".." || part.is_empty() || part == ".")
	{
		return Err("Model path must be asset-relative, use '/' separators, and contain no drive prefix or '..' components".into());
	}
	Ok(())
}

fn validate_name(name: &str) -> Result<(), String> {
	if name.trim().is_empty() || name.len() > 1024 || name.chars().any(char::is_control) {
		return Err(
			"Name must contain visible text, at most 1024 bytes, and no control characters".into(),
		);
	}
	Ok(())
}

impl CameraData {
	pub(crate) fn clear_removed_targets(&mut self, removed: &HashSet<EntityKey>) {
		if self
			.follow
			.as_ref()
			.is_some_and(|follow| removed.contains(&follow.target))
		{
			self.follow = None;
		}
		if self
			.orbit
			.as_ref()
			.is_some_and(|orbit| removed.contains(&orbit.target))
		{
			self.orbit = None;
		}
	}

	pub fn validate(&self) -> Result<(), String> {
		if self.follow.is_some() && self.orbit.is_some() {
			return Err("Choose either Follow or Orbit camera mode, not both".into());
		}

		if let Some(follow) = &self.follow {
			follow.validate()?;
		}

		if let Some(orbit) = &self.orbit {
			orbit.validate()?;
			orbit.validate_projection(self.near_plane, self.vertical_fov)?;
		}
		if !self
			.position
			.iter()
			.chain(
				[
					self.yaw,
					self.pitch,
					self.vertical_fov,
					self.near_plane,
					self.far_plane,
				]
				.iter(),
			)
			.all(|v| v.is_finite())
		{
			return Err("Camera values must all be finite".into());
		}
		let pitch_limit = 89.0_f32.to_radians();
		if !(-pitch_limit..=pitch_limit).contains(&self.pitch) {
			return Err("Camera pitch must be within ±89 degrees".into());
		}
		if self.vertical_fov <= 0.0 || self.vertical_fov >= std::f32::consts::PI {
			return Err("Camera vertical FOV must be between 0 and PI radians".into());
		}
		if self.near_plane <= 0.0 {
			return Err("Camera near plane must be greater than zero".into());
		}
		if self.far_plane <= self.near_plane {
			return Err("Camera far plane must be greater than the near plane".into());
		}
		let camera = crate::Camera {
			position: self.position,
			yaw: self.yaw,
			pitch: self.pitch,
			vertical_fov: self.vertical_fov,
			near: self.near_plane,
			far: self.far_plane,
		};
		if (glam::Vec3::from_array(camera.target()) - glam::Vec3::from_array(camera.position))
			.try_normalize()
			.is_none()
		{
			return Err(
				"Camera position cannot represent its viewing direction with f32 precision".into(),
			);
		}
		let matrix = camera.view_projection(1.0)?;
		if !matrix.is_finite() || matrix.determinant() == 0.0 || !matrix.inverse().is_finite() {
			return Err(
				"Camera settings cannot produce a finite invertible view/projection matrix".into(),
			);
		}
		crate::visibility::Frustum::from_view_projection(matrix).map_err(|error| {
			format!("Camera settings cannot produce a finite view frustum: {error}")
		})?;
		Ok(())
	}
}
