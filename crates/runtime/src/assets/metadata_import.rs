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

use super::{
	ImportedScene,
	gltf_loader::GltfLoaderError,
	import_budget::{ImportBudget, reject},
	scene_import::{self, ImportedMaterial, ImportedMesh, ImportedNode, ImportedPrimitiveMetadata},
};

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
		reference.validate()?;
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

pub(super) fn load_scene_metadata(
	path: &Path,
	root: &Path,
	mesh_index: Option<usize>,
	context: Option<&AssetJobContext>,
) -> Result<ImportedScene, GltfLoaderError> {
	let root = root.canonicalize().map_err(reject)?;
	let path = path.canonicalize().map_err(reject)?;
	if !path.starts_with(&root) {
		return Err(reject("model escapes asset root"));
	}
	let base = path
		.parent()
		.ok_or_else(|| reject("missing model directory"))?;
	let mut budget = ImportBudget::new(context);
	let bytes = budget.read_file(&path)?;
	let binary_bytes = if bytes.starts_with(b"glTF") {
		let binary = gltf::binary::Glb::from_slice(&bytes)?;
		budget.preflight_json(binary.json.as_ref())?;
		binary.bin.as_ref().map_or(0, |bytes| bytes.len())
	} else {
		budget.preflight_json(&bytes)?;
		0
	};
	// Gltf owns a copy of a GLB binary chunk. Charge before its parser allocates.
	budget.reserve(binary_bytes)?;
	let gltf::Gltf { document, mut blob } = gltf::Gltf::from_slice(&bytes)?;
	drop(bytes);
	budget.checkpoint()?;
	reserve_descriptors(&document, &mut budget)?;
	budget.reserve_product(
		document.buffers().len(),
		std::mem::size_of::<gltf::buffer::Data>(),
	)?;
	let mut buffers = Vec::with_capacity(document.buffers().len());
	for buffer in document.buffers() {
		budget.checkpoint()?;
		if buffer.length() > 64 * 1024 * 1024 {
			return Err(reject("buffer exceeds 64 MiB"));
		}
		let data = match buffer.source() {
			gltf::buffer::Source::Bin => blob
				.take()
				.ok_or_else(|| reject("missing GLB binary buffer"))?,
			gltf::buffer::Source::Uri(uri) => read_uri(base, &root, uri, &mut budget)?,
		};
		if data.len() < buffer.length() {
			return Err(reject("buffer is shorter than declared"));
		}
		buffers.push(gltf::buffer::Data(data));
	}
	let checkpoint = || context.map_or(Ok(()), |context| context.check_cancelled().map_err(reject));
	let mut scene = if let Some(index) = mesh_index {
		scene_import::describe_mesh_checked(&document, &buffers, index, &checkpoint)?
	} else if context.is_some() {
		scene_import::describe_scene_checked(&document, &buffers, &checkpoint)?
	} else {
		scene_import::describe_scene(&document, &buffers)?
	};
	// Validate/account images used by selected materials without decoding pixel
	// arrays or retaining URI strings. Unselected scene resources remain unused.
	let used_images: std::collections::HashSet<_> = scene
		.materials
		.iter()
		.filter_map(|material| material.base_color_image_index)
		.collect();
	budget.reserve_product(used_images.len(), 64)?;
	for image in document
		.images()
		.filter(|image| used_images.contains(&image.index()))
	{
		budget.checkpoint()?;
		match image.source() {
			gltf::image::Source::Uri { uri, .. } if uri.starts_with("data:") => {
				validate_data_uri(uri, &budget)?
			}
			gltf::image::Source::Uri { uri, .. } => {
				let path = resolve_uri(base, &root, uri)?;
				let file = std::fs::File::open(&path).map_err(reject)?;
				let metadata = file.metadata().map_err(reject)?;
				let size =
					usize::try_from(metadata.len()).map_err(|_| reject("image size overflow"))?;
				if !metadata.is_file() || size > 64 * 1024 * 1024 {
					return Err(reject(
						"image resource must be a regular file within 64 MiB",
					));
				}
				budget.reference(&path, size)?;
			}
			gltf::image::Source::View { view, .. } => {
				let end = view
					.offset()
					.checked_add(view.length())
					.ok_or_else(|| reject("image view size overflow"))?;
				if buffers
					.get(view.buffer().index())
					.and_then(|buffer| buffer.0.get(view.offset()..end))
					.is_none()
				{
					return Err(reject("image view lies outside its buffer"));
				}
			}
		}
	}
	scene.referenced_bytes = budget.referenced_bytes;
	if scene.retained_bytes()
		> context.map_or(
			super::gltf_loader::IMPORT_LIMIT,
			AssetJobContext::byte_limit,
		) {
		return Err(reject("retained metadata exceeds the job byte reservation"));
	}
	budget.checkpoint()?;
	Ok(scene)
}

fn reserve_descriptors(
	document: &gltf::Document,
	budget: &mut ImportBudget<'_>,
) -> Result<(), GltfLoaderError> {
	budget.reserve_product(
		document.nodes().len(),
		2 * std::mem::size_of::<ImportedNode>() + 256,
	)?;
	budget.reserve_product(
		document.meshes().len(),
		2 * std::mem::size_of::<ImportedMesh>() + 2 * std::mem::size_of::<Option<Bounds3>>() + 64,
	)?;
	budget.reserve_product(
		document.materials().len() + 1,
		2 * std::mem::size_of::<ImportedMaterial>() + 64,
	)?;
	for node in document.nodes() {
		let name = node.name().unwrap_or("Node");
		if name.len() > 1024 {
			return Err(reject("node name exceeds 1024 bytes"));
		}
		budget.reserve(name.len().saturating_mul(2) + 32)?;
	}
	for mesh in document.meshes() {
		budget.checkpoint()?;
		budget.reserve_product(
			mesh.primitives().len(),
			2 * std::mem::size_of::<ImportedPrimitiveMetadata>() + 32,
		)?;
		let name = mesh.name().unwrap_or("Mesh");
		if name.len() > 1024 {
			return Err(reject("mesh name exceeds 1024 bytes"));
		}
		budget.reserve(name.len() * 2)?;
	}
	for material in document.materials() {
		let name = material.name().unwrap_or("Material");
		if name.len() > 1024 {
			return Err(reject("material name exceeds 1024 bytes"));
		}
		budget.reserve(name.len() * 2)?;
	}
	Ok(())
}

fn resolve_uri(base: &Path, root: &Path, uri: &str) -> Result<PathBuf, GltfLoaderError> {
	if uri.len() > 16 * 1024 {
		return Err(reject("asset URI exceeds 16384 bytes"));
	}
	let decoded = urlencoding::decode(uri).map_err(reject)?;
	if decoded.contains([':', '\\', '\0']) || Path::new(decoded.as_ref()).is_absolute() {
		return Err(reject("only relative asset references are supported"));
	}
	let path = base.join(decoded.as_ref()).canonicalize().map_err(reject)?;
	if !path.starts_with(root) {
		return Err(reject("reference escapes asset root"));
	}
	Ok(path)
}

fn data_payload(uri: &str) -> Result<&str, GltfLoaderError> {
	uri.strip_prefix("data:")
		.and_then(|data| data.split_once(";base64,").map(|(_, data)| data))
		.ok_or_else(|| reject("unsupported data URI"))
}
fn read_uri(
	base: &Path,
	root: &Path,
	uri: &str,
	budget: &mut ImportBudget<'_>,
) -> Result<Vec<u8>, GltfLoaderError> {
	if uri.starts_with("data:") {
		let data = data_payload(uri)?;
		let maximum = data
			.len()
			.checked_div(4)
			.and_then(|bytes| bytes.checked_mul(3))
			.and_then(|bytes| bytes.checked_add(3))
			.ok_or_else(|| reject("data URI size overflow"))?;
		if maximum > 64 * 1024 * 1024 {
			return Err(reject("data URI exceeds 64 MiB"));
		}
		budget.reserve(maximum)?;
		let bytes = base64::decode(data).map_err(reject)?;
		budget.checkpoint()?;
		Ok(bytes)
	} else {
		let path = resolve_uri(base, root, uri)?;
		budget.read_file(&path)
	}
}

fn validate_data_uri(uri: &str, budget: &ImportBudget<'_>) -> Result<(), GltfLoaderError> {
	let encoded = data_payload(uri)?.as_bytes();
	let mut output = [0u8; 3072];
	for (index, chunk) in encoded.chunks(4096).enumerate() {
		budget.checkpoint()?;
		if (index + 1) * 4096 < encoded.len() && chunk.contains(&b'=') {
			return Err(reject("data URI padding precedes its end"));
		}
		base64::decode_config_slice(chunk, base64::STANDARD, &mut output).map_err(reject)?;
	}
	Ok(())
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
