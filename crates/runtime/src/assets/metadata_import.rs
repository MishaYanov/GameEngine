//! Concrete CPU metadata jobs. No renderer, device, mesh upload or image-pixel
//! decode occurs on this worker; completed descriptors contain no source buffers.
use std::path::{Path, PathBuf};

use engine_core::{
	Bounds3,
	asset_jobs::{
		AssetJobContext, AssetJobId, AssetJobLimits, AssetJobResult, AssetJobStats, AssetJobs,
		AssetSubmitError, CpuAssetImporter, ImportedAsset,
	},
	asset_registry::{AssetMetadata, AssetRef},
};

use super::ImportedScene;

mod decode;

pub(super) use decode::load_scene_metadata;

pub struct SceneMetadataImporter {
	root: PathBuf,
}
impl SceneMetadataImporter {
	pub fn new(asset_root: impl AsRef<Path>) -> Result<Self, String> {
		let metadata =
			std::fs::symlink_metadata(asset_root.as_ref()).map_err(|error| error.to_string())?;
		if metadata.file_type().is_symlink() || !metadata.is_dir() {
			return Err("Asset import root must be a directory without symbolic links".into());
		}
		Ok(Self {
			root: asset_root
				.as_ref()
				.canonicalize()
				.map_err(|error| error.to_string())?,
		})
	}
	pub fn asset_root(&self) -> &Path {
		&self.root
	}
}
impl CpuAssetImporter for SceneMetadataImporter {
	type Output = ImportedScene;
	fn import(
		&self,
		reference: &AssetRef,
		context: &AssetJobContext,
	) -> Result<ImportedAsset<Self::Output>, String> {
		let reference = super::project_assets::resolve_asset(&self.root, reference)?;
		let scene = load_scene_metadata(
			&self.root.join(&reference.path),
			&self.root,
			reference.mesh_index,
			Some(context),
		)
		.map_err(|error| error.to_string())?;
		context.check_cancelled()?;
		let bounds = scene_bounds(&scene, reference.mesh_index)?;
		Ok(ImportedAsset {
			bytes: scene.retained_bytes(),
			metadata: AssetMetadata {
				bounds,
				estimated_bytes: scene.decoded_mesh_bytes as u64,
			},
			value: scene,
		})
	}
}

/// Reusable application adapter. The notifier wakes a host/render event loop;
/// callers adopt descriptors with try_drain and perform GPU work on their owner.
pub struct AssetImportService {
	root: PathBuf,
	jobs: AssetJobs<ImportedScene>,
}
impl AssetImportService {
	pub fn new(asset_root: impl AsRef<Path>, limits: AssetJobLimits) -> Result<Self, String> {
		Self::with_notifier(asset_root, limits, || {})
	}
	pub fn with_notifier(
		asset_root: impl AsRef<Path>,
		limits: AssetJobLimits,
		on_ready: impl Fn() + Send + 'static,
	) -> Result<Self, String> {
		let importer = SceneMetadataImporter::new(asset_root)?;
		let root = importer.root.clone();
		let jobs = AssetJobs::new_with_notifier(importer, limits, on_ready)?;
		Ok(Self { root, jobs })
	}
	pub fn asset_root(&self) -> &Path {
		&self.root
	}
	pub fn try_submit(
		&self,
		reference: AssetRef,
		reserved_bytes: usize,
	) -> Result<AssetJobId, AssetSubmitError> {
		self.jobs.try_submit(reference, reserved_bytes)
	}
	pub fn cancel(&self, id: AssetJobId) -> Result<bool, AssetSubmitError> {
		self.jobs.cancel(id)
	}
	pub fn generation(&self) -> u64 {
		self.jobs.generation()
	}
	pub fn advance_generation(&self) -> Result<u64, AssetSubmitError> {
		self.jobs.advance_generation()
	}
	pub fn try_drain(&mut self) -> Option<AssetJobResult<ImportedScene>> {
		self.jobs.try_drain()
	}
	pub fn try_stats(&self) -> Option<AssetJobStats> {
		self.jobs.try_stats()
	}
}

fn scene_bounds(scene: &ImportedScene, mesh: Option<usize>) -> Result<Option<Bounds3>, String> {
	if let Some(mesh) = mesh {
		return Ok(scene.mesh_bounds.get(mesh).copied().flatten());
	}
	let mut bounds: Option<Bounds3> = None;
	for node in &scene.nodes {
		if let Some(local) = node
			.mesh_index
			.and_then(|index| scene.mesh_bounds.get(index).copied().flatten())
		{
			let world = local.transformed(glam::Mat4::from_cols_array(&node.world_matrix))?;
			bounds = Some(bounds.map_or(world, |bounds| bounds.union(world)));
		}
	}
	Ok(bounds)
}
