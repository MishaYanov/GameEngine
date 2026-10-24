//! CPU asset identity and metadata. Handles never carry renderer/device objects.
use std::{
	collections::{HashMap, HashSet},
	sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};

use crate::{Bounds3, scene_data::validate_model_path};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetRef {
	pub path: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub mesh_index: Option<usize>,
}

impl AssetRef {
	pub fn new(path: impl Into<String>, mesh_index: Option<usize>) -> Result<Self, String> {
		Self {
			path: path.into(),
			mesh_index,
		}
		.canonical()
	}
	/// Compact direct Rust inputs as well as validating their logical form.
	/// A short path must not retain an arbitrarily oversized String allocation.
	pub fn canonical(mut self) -> Result<Self, String> {
		self.validate()?;
		self.path = self.path.into_boxed_str().into_string();
		Ok(self)
	}
	/// Canonical logical references use '/' and never contain '.' or '..'.
	/// Filesystem canonicalization/containment belongs to the importer adapter.
	pub fn validate(&self) -> Result<(), String> {
		validate_model_path(&self.path)?;
		if self.mesh_index.is_some_and(|index| index > 1_000_000) {
			return Err("Asset mesh index exceeds 1000000".into());
		}
		Ok(())
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetHandle {
	registry: u64,
	index: u32,
	generation: u32,
}
impl AssetHandle {
	pub fn registry(self) -> u64 {
		self.registry
	}
	pub fn index(self) -> u32 {
		self.index
	}
	pub fn generation(self) -> u32 {
		self.generation
	}
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AssetMetadata {
	pub bounds: Option<Bounds3>,
	/// Estimated decoded payload size, independent of any GPU allocation.
	pub estimated_bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssetRecord {
	pub reference: AssetRef,
	pub metadata: AssetMetadata,
}

#[derive(Debug, Clone, Copy)]
pub struct AssetRegistryLimits {
	pub max_assets: usize,
	pub max_metadata_bytes: usize,
	pub max_estimated_bytes: u64,
}
impl Default for AssetRegistryLimits {
	fn default() -> Self {
		Self {
			max_assets: 4096,
			max_metadata_bytes: 4 * 1024 * 1024,
			max_estimated_bytes: 512 * 1024 * 1024,
		}
	}
}

#[derive(Clone)]
struct Slot {
	generation: u32,
	record: Option<AssetRecord>,
}

pub struct AssetRegistry {
	identity: u64,
	limits: AssetRegistryLimits,
	slots: Vec<Slot>,
	references: HashMap<AssetRef, u32>,
	free: Vec<u32>,
	metadata_bytes: usize,
	estimated_bytes: u64,
}

impl Default for AssetRegistry {
	fn default() -> Self {
		Self::new(AssetRegistryLimits::default()).expect("default asset limits are valid")
	}
}

impl AssetRegistry {
	pub fn new(limits: AssetRegistryLimits) -> Result<Self, String> {
		if limits.max_assets == 0
			|| limits.max_assets > 100_000
			|| limits.max_metadata_bytes == 0
			|| limits.max_metadata_bytes > 64 * 1024 * 1024
			|| limits.max_estimated_bytes == 0
			|| limits.max_estimated_bytes > 4 * 1024 * 1024 * 1024
		{
			return Err("Asset registry limits exceed the supported bounds".into());
		}
		static NEXT_REGISTRY: AtomicU64 = AtomicU64::new(1);
		let identity = NEXT_REGISTRY
			.fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
				value.checked_add(1)
			})
			.map_err(|_| "Asset registry identity exhausted")?;
		Ok(Self {
			identity,
			limits,
			slots: Vec::new(),
			references: HashMap::new(),
			free: Vec::new(),
			metadata_bytes: 0,
			estimated_bytes: 0,
		})
	}

	/// Updating metadata for the same reference preserves its handle. Removing
	/// and registering again changes the generation, rejecting stale handles.
	pub fn register(
		&mut self,
		reference: AssetRef,
		metadata: AssetMetadata,
	) -> Result<AssetHandle, String> {
		let reference = reference.canonical()?;
		let cost = metadata_cost(&reference)?;
		if let Some(&index) = self.references.get(&reference) {
			let slot = &mut self.slots[index as usize];
			let old = slot
				.record
				.as_ref()
				.ok_or("Asset registry index is inconsistent")?;
			let bytes = self
				.estimated_bytes
				.checked_sub(old.metadata.estimated_bytes)
				.and_then(|bytes| bytes.checked_add(metadata.estimated_bytes))
				.ok_or("Asset byte estimate overflow")?;
			if bytes > self.limits.max_estimated_bytes {
				return Err("Asset registry decoded byte budget exceeded".into());
			}
			slot.record = Some(AssetRecord {
				reference,
				metadata,
			});
			self.estimated_bytes = bytes;
			return Ok(AssetHandle {
				registry: self.identity,
				index,
				generation: slot.generation,
			});
		}
		if self.references.len() >= self.limits.max_assets
			|| self
				.metadata_bytes
				.checked_add(cost)
				.is_none_or(|bytes| bytes > self.limits.max_metadata_bytes)
			|| self
				.estimated_bytes
				.checked_add(metadata.estimated_bytes)
				.is_none_or(|bytes| bytes > self.limits.max_estimated_bytes)
		{
			return Err("Asset registry count or byte budget exceeded".into());
		}
		let index = match self.free.pop() {
			Some(index) => index,
			None => {
				if self.slots.len() >= self.limits.max_assets {
					return Err("Asset registry handle generations are exhausted".into());
				}
				let index = self.slots.len() as u32;
				self.slots.push(Slot {
					generation: 1,
					record: None,
				});
				index
			}
		};
		self.references.insert(reference.clone(), index);
		self.metadata_bytes += cost;
		self.estimated_bytes += metadata.estimated_bytes;
		let slot = &mut self.slots[index as usize];
		slot.record = Some(AssetRecord {
			reference,
			metadata,
		});
		Ok(AssetHandle {
			registry: self.identity,
			index,
			generation: slot.generation,
		})
	}

	/// Atomically register a bounded batch, returning handles in input order.
	/// Duplicate logical references are rejected rather than updated twice.
	/// Existing references preserve their handles; failures leave this registry
	/// and all of its byte/count accounting unchanged.
	pub fn register_batch(
		&mut self,
		entries: Vec<(AssetRef, AssetMetadata)>,
	) -> Result<Vec<AssetHandle>, String> {
		let (staged, handles) = self.staged_with_batch(entries)?;
		*self = staged;
		Ok(handles)
	}

	/// Internal staging retains identity only until the engine commits the
	/// paired authoring/metadata transaction. It must not expose a public clone
	/// that could independently issue aliasing handles for different assets.
	pub(crate) fn staged_with_batch(
		&self,
		entries: Vec<(AssetRef, AssetMetadata)>,
	) -> Result<(Self, Vec<AssetHandle>), String> {
		if entries.len() > self.limits.max_assets {
			return Err("Asset registration batch exceeds the registry count limit".into());
		}
		let mut canonical = Vec::with_capacity(entries.len());
		let mut staging_bytes = 0usize;
		for (reference, metadata) in entries {
			let reference = reference.canonical()?;
			staging_bytes = staging_bytes
				.checked_add(metadata_cost(&reference)?)
				.filter(|bytes| *bytes <= self.limits.max_metadata_bytes)
				.ok_or("Asset registration batch exceeds the metadata budget")?;
			canonical.push(Some((reference, metadata)));
		}
		let mut seen = HashSet::with_capacity(canonical.len());
		let mut projected_count = self.len();
		let mut projected_metadata = self.metadata_bytes;
		let mut projected_estimated = self.estimated_bytes;
		for entry in &canonical {
			let (reference, _) = entry.as_ref().ok_or("Missing staged asset")?;
			if !seen.insert(reference) {
				return Err(format!(
					"Duplicate asset reference in registration batch: '{}' mesh {:?}",
					reference.path, reference.mesh_index
				));
			}
			if let Some(handle) = self.find(reference) {
				let old = self
					.get(handle)
					.ok_or("Asset registry index is inconsistent")?;
				projected_estimated = projected_estimated
					.checked_sub(old.metadata.estimated_bytes)
					.ok_or("Asset byte estimate underflow")?;
			} else {
				projected_count += 1;
				projected_metadata = projected_metadata
					.checked_add(metadata_cost(reference)?)
					.ok_or("Asset metadata size overflow")?;
			}
		}
		for entry in &canonical {
			projected_estimated = projected_estimated
				.checked_add(
					entry
						.as_ref()
						.ok_or("Missing staged asset")?
						.1
						.estimated_bytes,
				)
				.ok_or("Asset byte estimate overflow")?;
		}
		if projected_count > self.limits.max_assets
			|| projected_metadata > self.limits.max_metadata_bytes
			|| projected_estimated > self.limits.max_estimated_bytes
		{
			return Err("Asset registry count or byte budget exceeded".into());
		}
		drop(seen);

		let mut staged = self.clone_for_staging();
		let mut handles = vec![None; canonical.len()];
		// Apply reductions first so a valid final total is independent of the
		// order of increases and decreases in the caller's batch.
		for (index, entry) in canonical.iter_mut().enumerate() {
			let decreases = entry.as_ref().is_some_and(|(reference, metadata)| {
				staged
					.find(reference)
					.and_then(|handle| staged.get(handle))
					.is_some_and(|old| metadata.estimated_bytes < old.metadata.estimated_bytes)
			});
			if decreases {
				let (reference, metadata) = entry.take().ok_or("Missing staged asset")?;
				handles[index] = Some(staged.register(reference, metadata)?);
			}
		}
		for (index, entry) in canonical.into_iter().enumerate() {
			if let Some((reference, metadata)) = entry {
				handles[index] = Some(staged.register(reference, metadata)?);
			}
		}
		let handles = handles
			.into_iter()
			.map(|handle| handle.ok_or_else(|| "Missing staged asset handle".to_owned()))
			.collect::<Result<Vec<_>, _>>()?;
		Ok((staged, handles))
	}

	fn clone_for_staging(&self) -> Self {
		Self {
			identity: self.identity,
			limits: self.limits,
			slots: self.slots.clone(),
			references: self.references.clone(),
			free: self.free.clone(),
			metadata_bytes: self.metadata_bytes,
			estimated_bytes: self.estimated_bytes,
		}
	}

	pub fn get(&self, handle: AssetHandle) -> Option<&AssetRecord> {
		if handle.registry != self.identity {
			return None;
		}
		let slot = self.slots.get(handle.index as usize)?;
		(slot.generation == handle.generation)
			.then_some(slot.record.as_ref())
			.flatten()
	}
	pub fn find(&self, reference: &AssetRef) -> Option<AssetHandle> {
		let index = *self.references.get(reference)?;
		Some(AssetHandle {
			registry: self.identity,
			index,
			generation: self.slots[index as usize].generation,
		})
	}
	pub fn remove(&mut self, handle: AssetHandle) -> Option<AssetRecord> {
		if handle.registry != self.identity {
			return None;
		}
		let slot = self.slots.get_mut(handle.index as usize)?;
		if slot.generation != handle.generation {
			return None;
		}
		let record = slot.record.take()?;
		self.references.remove(&record.reference);
		self.metadata_bytes -= metadata_cost(&record.reference).ok()?;
		self.estimated_bytes -= record.metadata.estimated_bytes;
		// A saturated slot is retired instead of wrapping into an old handle.
		if let Some(generation) = slot.generation.checked_add(1) {
			slot.generation = generation;
			self.free.push(handle.index);
		}
		Some(record)
	}
	pub fn clear(&mut self) {
		for index in 0..self.slots.len() {
			let handle = AssetHandle {
				registry: self.identity,
				index: index as u32,
				generation: self.slots[index].generation,
			};
			self.remove(handle);
		}
	}
	pub fn len(&self) -> usize {
		self.references.len()
	}
	pub fn is_empty(&self) -> bool {
		self.references.is_empty()
	}
	pub fn metadata_bytes(&self) -> usize {
		self.metadata_bytes
	}
	pub fn estimated_bytes(&self) -> u64 {
		self.estimated_bytes
	}
	pub fn iter(&self) -> impl Iterator<Item = (AssetHandle, &AssetRecord)> {
		self.slots.iter().enumerate().filter_map(|(index, slot)| {
			slot.record.as_ref().map(|record| {
				(
					AssetHandle {
						registry: self.identity,
						index: index as u32,
						generation: slot.generation,
					},
					record,
				)
			})
		})
	}
}

fn metadata_cost(reference: &AssetRef) -> Result<usize, String> {
	reference
		.path
		.len()
		.checked_mul(2)
		.and_then(|bytes| bytes.checked_add(192))
		.ok_or_else(|| "Asset metadata size overflow".into())
}
