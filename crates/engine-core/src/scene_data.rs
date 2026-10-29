use crate::{Transform, entity_key::EntityKey};
use glam::{EulerRot, Mat4};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const CURRENT_SCHEMA_VERSION: u32 = 3;
pub const MAX_SCENE_ENTITIES: usize = 100_000;
pub const MAX_SCENE_LIGHTS: usize = 16;
pub const MAX_SCENE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneData {
	pub schema_version: u32,
	pub bindings: BTreeMap<String, EntityKey>,
	pub entities: Vec<EntityData>,
	pub camera: CameraData,
	#[serde(default)]
	pub lights: Vec<LightData>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityData {
	pub key: EntityKey,
	pub name: String,
	pub model_path: String,
	pub transform: TransformData,
	#[serde(default = "default_renderable")]
	pub renderable: bool,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub parent: Option<EntityKey>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub material: Option<MaterialOverrideData>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub collider: Option<ColliderData>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub mesh_index: Option<usize>,
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
			transform: TransformData::default(),
			renderable: true,
			parent: None,
			material: None,
			collider: None,
			mesh_index: None,
		}
	}
	pub fn with_transform(mut self, transform: TransformData) -> Self {
		self.transform = transform;
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialOverrideData {
	pub base_color_factor: [f32; 4],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ColliderData {
	Box {
		half_extents: [f32; 3],
		offset: [f32; 3],
	},
	Sphere {
		radius: f32,
		offset: [f32; 3],
	},
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

#[derive(Deserialize)]
struct VersionHeader {
	schema_version: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SceneDataV1 {
	schema_version: u32,
	entities: Vec<EntityDataV1>,
	camera: CameraData,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntityDataV1 {
	name: String,
	model_path: String,
	transform: TransformData,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SceneDataV2 {
	schema_version: u32,
	bindings: BTreeMap<String, EntityKey>,
	entities: Vec<EntityDataV2>,
	camera: CameraData,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntityDataV2 {
	key: EntityKey,
	name: String,
	model_path: String,
	transform: TransformData,
}

pub fn parse_json(source: &str) -> Result<ParsedScene, String> {
	if source.len() > MAX_SCENE_BYTES {
		return Err("Scene JSON exceeds the 32 MiB size limit".into());
	}
	let header = serde_json::from_str::<VersionHeader>(source)
		.map_err(|error| format!("Invalid scene JSON: {error}"))?;
	let parsed = match header.schema_version {
		1 => {
			let legacy = serde_json::from_str::<SceneDataV1>(source)
				.map_err(|error| format!("Invalid version-1 scene JSON: {error}"))?;
			let entities = legacy
				.entities
				.into_iter()
				.enumerate()
				.map(|(index, entity)| {
					Ok(EntityData::new(
						EntityKey::new(format!("legacy-entity-{index}"))?,
						entity.name,
						entity.model_path,
					)
					.with_transform(entity.transform))
				})
				.collect::<Result<Vec<_>, String>>()?;
			let lights = legacy_lights(&entities)?;
			ParsedScene {
				source_schema_version: legacy.schema_version,
				data: SceneData {
					schema_version: CURRENT_SCHEMA_VERSION,
					bindings: BTreeMap::new(),
					entities,
					camera: legacy.camera,
					lights,
				},
			}
		}
		2 => {
			let legacy = serde_json::from_str::<SceneDataV2>(source)
				.map_err(|error| format!("Invalid version-2 scene JSON: {error}"))?;
			let entities = legacy
				.entities
				.into_iter()
				.map(|entity| {
					EntityData::new(entity.key, entity.name, entity.model_path)
						.with_transform(entity.transform)
				})
				.collect::<Vec<_>>();
			let lights = legacy_lights(&entities)?;
			ParsedScene {
				source_schema_version: legacy.schema_version,
				data: SceneData {
					schema_version: CURRENT_SCHEMA_VERSION,
					bindings: legacy.bindings,
					entities,
					camera: legacy.camera,
					lights,
				},
			}
		}
		CURRENT_SCHEMA_VERSION => ParsedScene {
			source_schema_version: header.schema_version,
			data: serde_json::from_str::<SceneData>(source)
				.map_err(|error| format!("Invalid version-3 scene JSON: {error}"))?,
		},
		version => return Err(format!("Unsupported schema version: {version}")),
	};
	parsed.data.validate()?;
	Ok(parsed)
}

// Versions 1 and 2 predate authored lights and used the renderer's default.
// Persist that appearance explicitly on migration; an empty v3 light list is
// intentional and receives only the renderer's ambient contribution.
fn legacy_lights(entities: &[EntityData]) -> Result<Vec<LightData>, String> {
	let occupied: HashSet<_> = entities.iter().map(|entity| entity.key.as_str()).collect();
	for index in 0..=entities.len() {
		let candidate = if index == 0 {
			"legacy-default-directional-light".to_string()
		} else {
			format!("legacy-default-directional-light-{index}")
		};
		if !occupied.contains(candidate.as_str()) {
			return Ok(vec![LightData::new(
				EntityKey::new(candidate)?,
				"Default directional light",
				LightKind::Directional,
			)]);
		}
	}
	Err("Cannot allocate a migrated default light identity".into())
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
		self.camera.validate()?;
		Ok(world)
	}

	pub fn from_json(source: &str) -> Result<Self, String> {
		parse_json(source).map(|parsed| parsed.data)
	}
	pub fn to_json(&self) -> Result<String, String> {
		self.validate()?;
		let mut writer = SceneJsonWriter::new(Vec::new());
		serde_json::to_writer_pretty(&mut writer, self)
			.map_err(|error| format!("Failed to serialize scene: {error}"))?;
		String::from_utf8(writer.output).map_err(|error| format!("Failed to encode scene: {error}"))
	}
	/// Exact pretty-JSON length, with the same validation and size limit as `to_json`.
	pub fn serialized_size(&self) -> Result<usize, String> {
		self.validate()?;
		let mut writer = SceneJsonWriter::new(std::io::sink());
		serde_json::to_writer_pretty(&mut writer, self)
			.map_err(|error| format!("Failed to serialize scene: {error}"))?;
		Ok(writer.bytes)
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

// Stop serialization at the same limit accepted by the loader, instead of
// constructing a potentially huge String and rejecting it after allocation.
struct SceneJsonWriter<W> {
	output: W,
	bytes: usize,
}
impl<W> SceneJsonWriter<W> {
	fn new(output: W) -> Self {
		Self { output, bytes: 0 }
	}
}
impl<W: std::io::Write> std::io::Write for SceneJsonWriter<W> {
	fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
		// Atomic scene saves append one conventional trailing newline.
		if bytes.len() > (MAX_SCENE_BYTES - 1).saturating_sub(self.bytes) {
			return Err(std::io::Error::other(
				"Scene JSON exceeds the 32 MiB size limit",
			));
		}
		let written = self.output.write(bytes)?;
		self.bytes += written;
		Ok(written)
	}
	fn flush(&mut self) -> std::io::Result<()> {
		self.output.flush()
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

impl ColliderData {
	pub fn validate(&self) -> Result<(), String> {
		let (offset, valid_size) = match self {
			Self::Box {
				half_extents,
				offset,
			} => (
				offset,
				half_extents.iter().all(|v| v.is_finite() && *v > 0.0),
			),
			Self::Sphere { radius, offset } => (offset, radius.is_finite() && *radius > 0.0),
		};
		if !valid_size || !offset.iter().all(|v| v.is_finite()) {
			return Err("Collider extents/radius must be positive and all values finite".into());
		}
		Ok(())
	}
}

impl CameraData {
	pub fn validate(&self) -> Result<(), String> {
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

/// Linear-time indexed parent traversal shared by CPU scene data and GPU scenes.
pub fn resolve_world_matrices(
	local: &[Mat4],
	keys: &[Option<&EntityKey>],
	parents: &[Option<&EntityKey>],
) -> Result<Vec<Mat4>, String> {
	if local.len() != keys.len() || local.len() != parents.len() || local.len() > MAX_SCENE_ENTITIES
	{
		return Err(
			"Hierarchy arrays must have matching lengths within the scene entity limit".into(),
		);
	}
	let mut indices = HashMap::with_capacity(keys.len());
	for (index, key) in keys.iter().enumerate() {
		if let Some(key) = key
			&& indices.insert(key.as_str(), index).is_some()
		{
			return Err("Duplicate entity key in hierarchy".into());
		}
	}
	let mut parent_indices = Vec::with_capacity(local.len());
	for parent in parents {
		parent_indices.push(match parent {
			Some(key) => Some(
				*indices
					.get(key.as_str())
					.ok_or_else(|| format!("Parent key {:?} does not exist", key.as_str()))?,
			),
			None => None,
		});
	}
	let mut states = vec![0u8; local.len()];
	let mut world = vec![Mat4::IDENTITY; local.len()];
	let mut path = Vec::new();
	for index in 0..local.len() {
		if states[index] == 2 {
			continue;
		}
		let mut current = Some(index);
		while let Some(node) = current {
			match states[node] {
				2 => break,
				1 => return Err("Scene hierarchy contains a parent cycle".into()),
				_ => {
					states[node] = 1;
					path.push(node);
					current = parent_indices[node];
				}
			}
		}
		while let Some(node) = path.pop() {
			world[node] =
				parent_indices[node].map_or(local[node], |parent| world[parent] * local[node]);
			if !world[node].is_finite() {
				return Err("Hierarchy produces a nonfinite world transform".into());
			}
			states[node] = 2;
		}
	}
	Ok(world)
}

#[cfg(test)]
mod serialization_tests {
	use super::*;
	use std::io::Write;

	#[test]
	fn counting_and_buffered_writers_reserve_the_same_newline_byte() {
		let mut count = SceneJsonWriter::new(std::io::sink());
		let mut buffered = SceneJsonWriter::new(Vec::new());
		count.bytes = MAX_SCENE_BYTES - 2;
		buffered.bytes = MAX_SCENE_BYTES - 2;
		for writer in [&mut count as &mut dyn Write, &mut buffered] {
			writer.write_all(b"a").unwrap();
			assert_eq!(
				writer.write_all(b"b").unwrap_err().to_string(),
				"Scene JSON exceeds the 32 MiB size limit"
			);
		}
		assert_eq!(count.bytes, MAX_SCENE_BYTES - 1);
		assert_eq!(buffered.bytes, count.bytes);
		assert_eq!(buffered.output, b"a");
	}
}
