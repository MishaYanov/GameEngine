//! Allocation/resource preflight for CPU metadata jobs. The streaming JSON
//! pass charges conservative owned-document storage before glTF deserialization.
use std::{
	collections::HashSet,
	fmt,
	io::Read,
	path::{Path, PathBuf},
};

use engine_core::asset_jobs::AssetJobContext;
use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};

use super::gltf_loader::{GltfLoaderError, IMPORT_LIMIT};

pub(super) struct ImportBudget<'a> {
	remaining: usize,
	limit: usize,
	context: Option<&'a AssetJobContext>,
	references: HashSet<PathBuf>,
	pub referenced_bytes: usize,
	values: usize,
}
impl<'a> ImportBudget<'a> {
	pub fn new(context: Option<&'a AssetJobContext>) -> Self {
		let limit = context.map_or(IMPORT_LIMIT, |context| {
			context.byte_limit().min(IMPORT_LIMIT)
		});
		Self {
			remaining: limit,
			limit,
			context,
			references: HashSet::new(),
			referenced_bytes: 0,
			values: 0,
		}
	}
	pub fn checkpoint(&self) -> Result<(), GltfLoaderError> {
		if let Some(context) = self.context {
			context.check_cancelled().map_err(reject)?;
		}
		Ok(())
	}
	pub fn reserve(&mut self, bytes: usize) -> Result<(), GltfLoaderError> {
		self.checkpoint()?;
		self.remaining = self.remaining.checked_sub(bytes).ok_or_else(|| {
			reject(format!(
				"metadata import exceeds its {}-byte reservation",
				self.limit
			))
		})?;
		Ok(())
	}
	pub fn reserve_product(&mut self, count: usize, bytes: usize) -> Result<(), GltfLoaderError> {
		self.reserve(
			count
				.checked_mul(bytes)
				.ok_or_else(|| reject("metadata allocation size overflow"))?,
		)
	}
	pub fn reference(&mut self, path: &Path, bytes: usize) -> Result<(), GltfLoaderError> {
		if !self.references.contains(path) {
			self.referenced_bytes = self
				.referenced_bytes
				.checked_add(bytes)
				.ok_or_else(|| reject("referenced byte size overflow"))?;
			if self.referenced_bytes > self.limit {
				return Err(reject(
					"referenced resources exceed the job byte reservation",
				));
			}
			self.reserve(
				path.as_os_str()
					.len()
					.checked_mul(8)
					.and_then(|bytes| bytes.checked_add(128))
					.ok_or_else(|| reject("reference path size overflow"))?,
			)?;
			self.references.insert(path.to_path_buf());
		}
		Ok(())
	}
	pub fn read_file(&mut self, path: &Path) -> Result<Vec<u8>, GltfLoaderError> {
		self.checkpoint()?;
		let mut file = std::fs::File::open(path).map_err(reject)?;
		let metadata = file.metadata().map_err(reject)?;
		let size = usize::try_from(metadata.len()).map_err(|_| reject("resource size overflow"))?;
		if !metadata.is_file() || size > 64 * 1024 * 1024 {
			return Err(reject("resource must be a regular file within 64 MiB"));
		}
		self.reference(path, size)?;
		self.reserve(size)?;
		let mut bytes = Vec::with_capacity(size);
		let mut chunk = [0u8; 64 * 1024];
		while bytes.len() < size {
			self.checkpoint()?;
			let limit = (size - bytes.len()).min(chunk.len());
			let count = file.read(&mut chunk[..limit]).map_err(reject)?;
			if count == 0 {
				return Err(reject("resource shrank during import"));
			}
			bytes.extend_from_slice(&chunk[..count]);
		}
		let mut extra = [0u8; 1];
		if file.read(&mut extra).map_err(reject)? != 0 {
			return Err(reject("resource grew during import"));
		}
		Ok(bytes)
	}
	pub fn preflight_json(&mut self, json: &[u8]) -> Result<(), GltfLoaderError> {
		// Escaped strings may use serde's scratch buffer before a visitor runs.
		// Reserve that possible peak before asking the parser to scan tokens.
		self.reserve(json.len())?;
		let mut deserializer = serde_json::Deserializer::from_slice(json);
		Scan {
			budget: self,
			depth: 0,
			root_array: false,
		}
		.deserialize(&mut deserializer)
		.map_err(reject)?;
		deserializer.end().map_err(reject)?;
		self.checkpoint()
	}
}

pub(super) fn reject(message: impl ToString) -> GltfLoaderError {
	GltfLoaderError::Resource(message.to_string())
}

struct Scan<'a, 'b> {
	budget: &'a mut ImportBudget<'b>,
	depth: usize,
	root_array: bool,
}
impl<'de> DeserializeSeed<'de> for Scan<'_, '_> {
	type Value = ();
	fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
		if self.depth > 128 {
			return Err(serde::de::Error::custom("metadata JSON exceeds 128 levels"));
		}
		self.budget.values += 1;
		if self.budget.values > 1_000_000 {
			return Err(serde::de::Error::custom(
				"metadata JSON exceeds 1000000 values",
			));
		}
		self.budget.reserve(128).map_err(serde::de::Error::custom)?;
		deserializer.deserialize_any(self)
	}
}
impl<'de> Visitor<'de> for Scan<'_, '_> {
	type Value = ();
	fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("bounded glTF JSON")
	}
	fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<(), E> {
		Ok(())
	}
	fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<(), E> {
		Ok(())
	}
	fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<(), E> {
		Ok(())
	}
	fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<(), E> {
		Ok(())
	}
	fn visit_unit<E: serde::de::Error>(self) -> Result<(), E> {
		Ok(())
	}
	fn visit_none<E: serde::de::Error>(self) -> Result<(), E> {
		Ok(())
	}
	fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<(), E> {
		self.budget
			.reserve_product(value.len(), 2)
			.map_err(E::custom)
	}
	fn visit_string<E: serde::de::Error>(self, value: String) -> Result<(), E> {
		self.visit_str(&value)
	}
	fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<(), A::Error> {
		self.budget.reserve(512).map_err(serde::de::Error::custom)?;
		let mut count = 0;
		while sequence
			.next_element_seed(Scan {
				budget: self.budget,
				depth: self.depth + 1,
				root_array: false,
			})?
			.is_some()
		{
			count += 1;
			if self.root_array && count > 10_000 {
				return Err(serde::de::Error::custom(
					"glTF root array exceeds 10000 entries",
				));
			}
		}
		Ok(())
	}
	fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
		// Includes container capacity and allocator slack for owned glTF nodes.
		self.budget
			.reserve(1024)
			.map_err(serde::de::Error::custom)?;
		while let Some(key) = map.next_key::<String>()? {
			if key.len() > 4096 {
				return Err(serde::de::Error::custom(
					"glTF field name exceeds 4096 bytes",
				));
			}
			self.budget
				.reserve_product(key.len(), 2)
				.map_err(serde::de::Error::custom)?;
			self.budget.reserve(128).map_err(serde::de::Error::custom)?;
			map.next_value_seed(Scan {
				budget: self.budget,
				depth: self.depth + 1,
				root_array: self.depth == 0,
			})?;
		}
		Ok(())
	}
}
