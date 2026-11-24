//! Single-use, CPU-only decode snapshots for the editor warm worker.
use std::{
	collections::{HashMap, HashSet},
	io::Read,
	path::{Path, PathBuf},
	sync::atomic::{AtomicUsize, Ordering},
};

use engine_core::{
	asset_catalog::{AssetCatalog, AssetId, AssetKind, PersistentAsset},
	asset_registry::AssetRef,
};
use sha2::{Digest, Sha256};

use super::{
	gltf_loader::{ImportedPrimitive, load_warm_mesh},
	import_budget::ImportBudget,
	project_assets::{fingerprint, load_catalog_for_warming, open_warm_file, warm_model_path},
};

const MAX_WARM_BYTES: usize = 32 * 1024 * 1024;
const MAX_BATCH_BYTES: usize = 16 * 1024 * 1024;
const MAX_WARM_MODELS: usize = 8;
const BATCH_CONTAINER_BYTES: usize = 4 * 1024;
const SCRATCH_OVERHEAD_BYTES: usize = 128 * 1024;
static LIVE_WARM_BYTES: AtomicUsize = AtomicUsize::new(0);

struct Reservation<'a> {
	live: &'a AtomicUsize,
	bytes: usize,
}

impl<'a> Reservation<'a> {
	/// Reserve all currently available scratch. Ready batches still charge
	/// their live payloads, including during a workspace replacement.
	fn scratch(live: &'a AtomicUsize) -> Option<Self> {
		let previous = live
			.fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
				(used < MAX_WARM_BYTES).then_some(MAX_WARM_BYTES)
			})
			.ok()?;
		Some(Self {
			live,
			bytes: MAX_WARM_BYTES - previous,
		})
	}

	fn keep(&mut self, bytes: usize) {
		debug_assert!(bytes <= self.bytes);
		self.live.fetch_sub(self.bytes - bytes, Ordering::AcqRel);
		self.bytes = bytes;
	}
}

impl Drop for Reservation<'_> {
	fn drop(&mut self) {
		self.live.fetch_sub(self.bytes, Ordering::AcqRel);
	}
}

#[derive(PartialEq, Eq)]
struct WarmModelKey {
	canonical_path: PathBuf,
	asset_id: AssetId,
	mesh_index: usize,
	asset_revision: u64,
	fingerprint: String,
}

pub(super) struct CurrentModel {
	revision: u64,
	bytes: u64,
	fingerprint: String,
}

impl From<&PersistentAsset> for CurrentModel {
	fn from(asset: &PersistentAsset) -> Self {
		Self {
			revision: asset.revision,
			bytes: asset.bytes,
			fingerprint: asset.fingerprint.clone(),
		}
	}
}

struct WarmModel {
	key: WarmModelKey,
	source_bytes: u64,
	primitives: Vec<ImportedPrimitive>,
}

impl WarmModel {
	fn prepare(
		root: &Path,
		asset: &PersistentAsset,
		mesh_index: usize,
		budget: &mut ImportBudget<'_>,
	) -> Option<Self> {
		let source_limit = usize::try_from(asset.bytes).ok()?;
		budget.reserve(SCRATCH_OVERHEAD_BYTES).ok()?;
		let path = warm_model_path(root, &asset.path).ok()?;
		let bytes = budget.read_file_at_most(&path, source_limit).ok()?;
		let observed = fingerprint(&bytes);
		if bytes.len() != source_limit || observed != asset.fingerprint {
			return None;
		}
		let primitives = load_warm_mesh(bytes, observed, mesh_index, budget).ok()?;
		budget.checkpoint().ok()?;
		Some(Self {
			key: WarmModelKey {
				canonical_path: path,
				asset_id: asset.id.clone(),
				mesh_index,
				asset_revision: asset.revision,
				fingerprint: asset.fingerprint.clone(),
			},
			source_bytes: asset.bytes,
			primitives,
		})
	}

	fn retained_bytes(&self) -> usize {
		let mut images = HashSet::new();
		let mut bytes = self.key.canonical_path.capacity()
			+ self.key.asset_id.as_str().len()
			+ self.key.fingerprint.capacity()
			+ size_of::<ImportedPrimitive>() * self.primitives.capacity()
			+ 256;
		for primitive in &self.primitives {
			bytes += primitive.mesh.retained_bytes();
			if let Some((_, image)) = primitive
				.base_color_image_index
				.zip(primitive.material.base_color_image.as_ref())
				.filter(|(index, _)| images.insert(*index))
			{
				bytes += image.retained_bytes() + 128;
			}
		}
		bytes
	}
}

struct CatalogLookup<'a> {
	by_id: HashMap<&'a AssetId, &'a PersistentAsset>,
	by_path: HashMap<String, &'a PersistentAsset>,
}

impl<'a> CatalogLookup<'a> {
	fn new(catalog: &'a AssetCatalog, budget: &mut ImportBudget<'_>) -> Option<Self> {
		budget.reserve_product(catalog.assets.len(), 128).ok()?;
		let mut by_id = HashMap::with_capacity(catalog.assets.len());
		let mut by_path = HashMap::new();
		for asset in &catalog.assets {
			by_id.insert(&asset.id, asset);
			for path in std::iter::once(&asset.path).chain(&asset.aliases) {
				budget.reserve_product(path.len(), 4).ok()?;
				budget.reserve(128).ok()?;
				by_path.insert(path.to_lowercase(), asset);
			}
		}
		Some(Self { by_id, by_path })
	}

	fn static_model(&self, reference: &AssetRef) -> Option<(&'a PersistentAsset, usize)> {
		let asset = match &reference.asset_id {
			Some(id) => self.by_id.get(id),
			None => self.by_path.get(&reference.path.to_lowercase()),
		}?;
		let mesh_index = if let Some(mesh_id) = &reference.mesh_id {
			asset.mesh_by_id(mesh_id)?.index
		} else {
			let index = reference.mesh_index.unwrap_or(0);
			asset
				.meshes
				.iter()
				.any(|mesh| mesh.index == index)
				.then_some(index)?
		};
		(asset.kind == AssetKind::Model
			&& Path::new(&asset.path)
				.extension()
				.and_then(|extension| extension.to_str())
				.is_some_and(|extension| extension.eq_ignore_ascii_case("glb")))
		.then_some((*asset, mesh_index))
	}
}

/// One authored scene's individually matched, single-use model payloads.
/// The charge remains live through upload until the batch is dropped.
pub struct PreparedModelBatch {
	models: Vec<WarmModel>,
	consumed: usize,
	reservation: Reservation<'static>,
}

impl PreparedModelBatch {
	/// Attempt up to eight distinct managed static meshes in authored order.
	/// An unavailable reservation, unmanaged model, or failed preparation
	/// leaves that model on the ordinary loader path.
	pub fn prepare<'a>(
		asset_root: &Path,
		references: impl Iterator<Item = &'a AssetRef>,
		cancelled: &dyn Fn() -> bool,
	) -> Option<Self> {
		let mut reservation = Reservation::scratch(&LIVE_WARM_BYTES)?;
		let mut catalog_budget = ImportBudget::warming(reservation.bytes, cancelled);
		catalog_budget
			.reserve(BATCH_CONTAINER_BYTES + SCRATCH_OVERHEAD_BYTES)
			.ok()?;
		let catalog = load_catalog_for_warming(asset_root, &mut catalog_budget).ok()?;
		let lookup = CatalogLookup::new(&catalog, &mut catalog_budget)?;
		let mut retained = BATCH_CONTAINER_BYTES;
		let mut models = Vec::with_capacity(MAX_WARM_MODELS);
		let mut seen = HashSet::with_capacity(MAX_WARM_MODELS);
		for reference in references {
			catalog_budget.checkpoint().ok()?;
			let Some((asset, mesh_index)) = lookup.static_model(reference) else {
				continue;
			};
			if !seen.insert((&asset.id, mesh_index)) {
				continue;
			}
			let available = reservation
				.bytes
				.saturating_sub(catalog_budget.charged_bytes())
				.saturating_sub(retained - BATCH_CONTAINER_BYTES);
			let mut budget = ImportBudget::warming(available, cancelled);
			if let Some(model) = WarmModel::prepare(asset_root, asset, mesh_index, &mut budget) {
				let next = retained + model.retained_bytes();
				if next <= MAX_BATCH_BYTES && next <= reservation.bytes {
					retained = next;
					models.push(model);
				}
			}
			if seen.len() == MAX_WARM_MODELS {
				break;
			}
		}
		catalog_budget.checkpoint().ok()?;
		if models.is_empty() {
			return None;
		}
		drop(seen);
		drop(lookup);
		drop(catalog);
		reservation.keep(retained);
		Some(Self {
			models,
			consumed: 0,
			reservation,
		})
	}

	pub fn prepared_count(&self) -> usize {
		self.models.len() + self.consumed
	}

	pub fn consumed_count(&self) -> usize {
		self.consumed
	}

	pub fn retained_bytes(&self) -> usize {
		self.reservation.bytes
	}

	pub(super) fn take_current(
		&mut self,
		asset_root: &Path,
		reference: &AssetRef,
		current: &CurrentModel,
		path: &Path,
	) -> Option<(String, Vec<ImportedPrimitive>)> {
		let id = reference.asset_id.as_ref()?;
		let mesh_index = reference.mesh_index.unwrap_or(0);
		let index = self.models.iter().position(|model| {
			model.key.canonical_path == path
				&& &model.key.asset_id == id
				&& model.key.mesh_index == mesh_index
		})?;
		let model = &self.models[index];
		if model.key.asset_revision != current.revision
			|| model.key.fingerprint != current.fingerprint
			|| model.source_bytes != current.bytes
			|| warm_model_path(asset_root, &reference.path).ok()?.as_path() != path
			|| !matches_current_bytes(path, model.source_bytes, &model.key.fingerprint)
		{
			return None;
		}
		let model = self.models.swap_remove(index);
		self.consumed += 1;
		Some((model.key.fingerprint, model.primitives))
	}
}

fn matches_current_bytes(path: &Path, expected_bytes: u64, expected_sha256: &str) -> bool {
	let check = || -> Option<bool> {
		let mut file = open_warm_file(path).ok()?;
		let metadata = file.metadata().ok()?;
		if !metadata.is_file() || metadata.len() != expected_bytes {
			return Some(false);
		}
		let mut remaining = expected_bytes;
		let mut hash = Sha256::new();
		let mut chunk = [0u8; 16 * 1024];
		while remaining != 0 {
			let size = remaining.min(chunk.len() as u64) as usize;
			file.read_exact(&mut chunk[..size]).ok()?;
			hash.update(&chunk[..size]);
			remaining -= size as u64;
		}
		let mut extra = [0u8; 1];
		Some(
			file.read(&mut extra).ok()? == 0 && format!("{:x}", hash.finalize()) == expected_sha256,
		)
	};
	check() == Some(true)
}

#[cfg(test)]
#[path = "../../tests/unit/assets/warm_model.rs"]
mod tests;
