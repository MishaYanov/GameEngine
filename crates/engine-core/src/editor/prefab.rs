use super::EditorSession;
use crate::{
	EntityKey,
	scene_data::{CURRENT_SCHEMA_VERSION, CameraData, EntityData, SceneData, TransformData},
};
use serde::{Deserialize, Serialize};
use std::{
	collections::{BTreeMap, HashMap, HashSet},
	io::{self, Write},
};

pub const MAX_PREFAB_ENTITIES: usize = 4096;
pub const MAX_PREFAB_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrefabData {
	pub schema_version: u32,
	pub entities: Vec<EntityData>,
}

impl PrefabData {
	pub const MAX_ENTITIES: usize = MAX_PREFAB_ENTITIES;
	pub const MAX_JSON_BYTES: usize = MAX_PREFAB_BYTES;
	pub fn from_json(source: &str) -> Result<Self, String> {
		if source.len() > MAX_PREFAB_BYTES {
			return Err("Prefab JSON exceeds the 16 MiB size limit".into());
		}
		serde_json::from_str::<Self>(source)
			.map_err(|error| format!("Invalid prefab JSON: {error}"))?
			.validated()
	}

	pub fn validate(&self) -> Result<(), String> {
		self.clone().validated().map(|_| ())
	}

	pub fn to_json(&self) -> Result<String, String> {
		self.validate()?;
		let source = serde_json::to_string_pretty(self).map_err(|error| error.to_string())?;
		if source.len() > MAX_PREFAB_BYTES {
			return Err("Formatted prefab JSON exceeds the 16 MiB size limit".into());
		}
		Ok(source)
	}

	fn validated(self) -> Result<Self, String> {
		if self.schema_version != 1
			|| self.entities.is_empty()
			|| self.entities.len() > MAX_PREFAB_ENTITIES
		{
			return Err("A version-1 prefab requires 1–4096 entities".into());
		}
		// A byte-counting writer checks the owned DTO without allocating a
		// second JSON document. Direct bridge DTOs use the same size contract.
		serde_json::to_writer(SizeBudget(0), &self)
			.map_err(|error| format!("Prefab size validation failed: {error}"))?;
		let camera = crate::Camera::default();
		let validation = SceneData {
			schema_version: CURRENT_SCHEMA_VERSION,
			entities: self.entities,
			bindings: BTreeMap::new(),
			camera: CameraData {
				position: camera.position,
				yaw: camera.yaw,
				pitch: camera.pitch,
				vertical_fov: camera.vertical_fov,
				near_plane: camera.near,
				far_plane: camera.far,
			},
			lights: Vec::new(),
		};
		// The isolated scene rejects duplicate/invalid keys, external parents,
		// cycles, invalid paths and every material/collider/transform defect.
		validation.validate()?;
		Ok(Self {
			schema_version: 1,
			entities: validation.entities,
		})
	}
}

struct SizeBudget(usize);
impl Write for SizeBudget {
	fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
		self.0 = self
			.0
			.checked_add(bytes.len())
			.filter(|count| *count <= MAX_PREFAB_BYTES)
			.ok_or_else(|| {
				io::Error::new(
					io::ErrorKind::InvalidInput,
					"Prefab exceeds the 16 MiB size limit",
				)
			})?;
		Ok(bytes.len())
	}
	fn flush(&mut self) -> io::Result<()> {
		Ok(())
	}
}

impl EditorSession {
	pub fn export_prefab(&self) -> Result<PrefabData, String> {
		self.require_edit()?;
		self.require_no_transaction()?;
		if self.selection.is_empty() {
			return Err("Select entities to create a prefab".into());
		}
		let data = self.data.as_ref();
		let indices: HashMap<_, _> = data
			.entities
			.iter()
			.enumerate()
			.map(|(index, entity)| (&entity.key, index))
			.collect();
		let mut children = vec![Vec::new(); data.entities.len()];
		for (index, entity) in data.entities.iter().enumerate() {
			if let Some(parent) = &entity.parent {
				children[*indices
					.get(parent)
					.ok_or("Prefab source contains a missing parent")?]
				.push(index);
			}
		}
		let mut pending = self
			.selection
			.iter()
			.map(|key| {
				indices
					.get(key)
					.copied()
					.ok_or("Prefab selection contains a missing entity")
			})
			.collect::<Result<Vec<_>, _>>()?;
		let mut included = vec![false; data.entities.len()];
		let mut count = 0;
		while let Some(index) = pending.pop() {
			if included[index] {
				continue;
			}
			included[index] = true;
			count += 1;
			if count > MAX_PREFAB_ENTITIES {
				return Err("Selected prefab subtree exceeds 4096 entities".into());
			}
			pending.extend_from_slice(&children[index]);
		}
		let world = data.world_matrices()?;
		let mut entities = Vec::with_capacity(count);
		for (index, entity) in data.entities.iter().enumerate() {
			if !included[index] {
				continue;
			}
			let mut entity = entity.clone();
			if entity
				.parent
				.as_ref()
				.is_some_and(|parent| !included[indices[parent]])
			{
				entity.parent = None;
				entity.transform = TransformData::from_matrix(world[index])?;
			}
			entities.push(entity);
		}
		let roots: Vec<_> = entities
			.iter()
			.filter(|entity| entity.parent.is_none())
			.map(|entity| entity.transform.position)
			.collect();
		let count = roots.len().max(1) as f64;
		let pivot: [f32; 3] = std::array::from_fn(|axis| {
			(roots
				.iter()
				.map(|position| position[axis] as f64 / count)
				.sum::<f64>()) as f32
		});
		for entity in &mut entities {
			if entity.parent.is_none() {
				entity.transform.position = (glam::Vec3::from_array(entity.transform.position)
					- glam::Vec3::from_array(pivot))
				.to_array();
			}
		}
		PrefabData {
			schema_version: 1,
			entities,
		}
		.validated()
	}

	pub fn instantiate_prefab(
		&mut self,
		prefab: PrefabData,
		position: [f32; 3],
	) -> Result<crate::command::EditorSnapshotDto, String> {
		self.instantiate_prefab_command(prefab, position)?;
		self.snapshot()
	}

	pub fn instantiate_prefab_command(
		&mut self,
		prefab: PrefabData,
		position: [f32; 3],
	) -> Result<(), String> {
		self.require_edit()?;
		if !position.iter().all(|value| value.is_finite()) {
			return Err("Prefab position must be finite".into());
		}
		let prefab = prefab.validated()?;
		if self
			.data
			.entities
			.len()
			.checked_add(prefab.entities.len())
			.is_none_or(|count| count > crate::scene_data::MAX_SCENE_ENTITIES)
		{
			return Err("Instantiating this prefab would exceed the scene entity limit".into());
		}
		let mut used: HashSet<_> = self
			.data
			.entities
			.iter()
			.map(|entity| entity.key.clone())
			.chain(self.data.lights.iter().map(|light| light.key.clone()))
			.collect();
		let remap: HashMap<_, _> = prefab
			.entities
			.iter()
			.map(|entity| {
				let key = loop {
					let key = EntityKey::generate();
					if used.insert(key.clone()) {
						break key;
					}
				};
				(entity.key.clone(), key)
			})
			.collect();
		let mut entities = prefab.entities;
		let mut selection = Vec::new();
		for entity in &mut entities {
			entity.key = remap.get(&entity.key).ok_or("Missing prefab key")?.clone();
			entity.parent = entity
				.parent
				.as_ref()
				.map(|parent| {
					remap
						.get(parent)
						.cloned()
						.ok_or("Prefab references an external parent")
				})
				.transpose()?;
			if entity.parent.is_none() {
				entity.transform.position = (glam::Vec3::from_array(entity.transform.position)
					+ glam::Vec3::from_array(position))
				.to_array();
				if !entity
					.transform
					.position
					.iter()
					.all(|value| value.is_finite())
				{
					return Err(
						"Prefab placement exceeds the supported finite position range".into(),
					);
				}
				selection.push(entity.key.clone());
			}
		}
		// One command/transaction patch covers the whole expanded instance.
		// Semantic role bindings continue to refer to the original scene.
		self.replace_entities_command(entities, selection, "Instantiate prefab")
	}
}
