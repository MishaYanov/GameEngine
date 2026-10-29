use std::{
	collections::{HashMap, HashSet, VecDeque},
	error::Error,
	fmt::{Display, Formatter},
	io,
	path::{Path, PathBuf},
	sync::{Arc, Weak},
};

use renderer::vulkan::{
	Vertex, VulkanMaterial, VulkanMesh, VulkanMeshError, VulkanTexture, VulkanTextureError,
};

use renderer::WindowRenderer;

use super::{ImageAsset, ImageAssetError, MaterialData, MeshData};
use super::{
	Model,
	gltf_loader::{
		self, GltfLoaderError, ImportedPrimitive, Resources, import_mesh, load_resources_in_root,
	},
	model::ModelPart,
};

pub struct AssetManager {
	root: PathBuf,
	textures: HashMap<PathBuf, Arc<VulkanTexture>>,
	meshes: HashMap<String, Arc<VulkanMesh>>,
	image_textures: HashMap<String, Arc<VulkanTexture>>,
	models: HashMap<engine_core::asset_registry::AssetRef, Arc<Model>>,
	scenes: HashMap<PathBuf, Arc<super::ImportedScene>>,
	scene_order: VecDeque<PathBuf>,
	scene_bytes: usize,
	resource_cache: HashMap<PathBuf, (Resources, usize)>,
	resource_order: VecDeque<PathBuf>,
	resource_bytes: usize,
	gpu_allocations: Vec<GpuAllocation>,
	gpu_bytes: usize,
}

enum GpuReference {
	Mesh(Weak<VulkanMesh>),
	Texture(Weak<VulkanTexture>),
}
struct GpuAllocation {
	resource: GpuReference,
	bytes: usize,
}
impl GpuAllocation {
	fn live(&self) -> bool {
		match &self.resource {
			GpuReference::Mesh(resource) => resource.strong_count() > 0,
			GpuReference::Texture(resource) => resource.strong_count() > 0,
		}
	}
}

#[derive(Debug)]
pub enum AssetManagerError {
	Io(io::Error),
	Image(ImageAssetError),
	Texture(VulkanTextureError),
	Mesh(VulkanMeshError),
	Gltf(GltfLoaderError),
	OutsideAssetRoot { path: PathBuf, root: PathBuf },
	ResourceBudget(String),
	Upload(String),
}

impl Display for AssetManagerError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Io(error) => {
				write!(formatter, "asset I/O error: {error}",)
			}

			Self::Image(error) => {
				write!(formatter, "image asset error: {error}",)
			}

			Self::Texture(error) => {
				write!(formatter, "Vulkan texture asset error: {error}",)
			}

			Self::Mesh(error) => {
				write!(formatter, "Vulkan mesh asset error: {error}",)
			}

			Self::Gltf(error) => write!(formatter, "model import error: {error}"),
			Self::OutsideAssetRoot { path, root } => {
				write!(
					formatter,
					"asset path '{}' is outside asset root '{}'",
					path.display(),
					root.display(),
				)
			}
			Self::ResourceBudget(message) => {
				write!(formatter, "GPU asset budget exceeded: {message}")
			}
			Self::Upload(message) => write!(formatter, "GPU asset upload failed: {message}"),
		}
	}
}

impl Error for AssetManagerError {
	fn source(&self) -> Option<&(dyn Error + 'static)> {
		match self {
			Self::Io(error) => Some(error),
			Self::Image(error) => Some(error),
			Self::Texture(error) => Some(error),
			Self::Mesh(error) => Some(error),
			Self::Gltf(error) => Some(error),
			Self::OutsideAssetRoot { .. } | Self::ResourceBudget(_) | Self::Upload(_) => None,
		}
	}
}

impl From<GltfLoaderError> for AssetManagerError {
	fn from(value: GltfLoaderError) -> Self {
		Self::Gltf(value)
	}
}

impl From<io::Error> for AssetManagerError {
	fn from(value: io::Error) -> Self {
		Self::Io(value)
	}
}

impl From<ImageAssetError> for AssetManagerError {
	fn from(value: ImageAssetError) -> Self {
		Self::Image(value)
	}
}

impl From<VulkanTextureError> for AssetManagerError {
	fn from(value: VulkanTextureError) -> Self {
		Self::Texture(value)
	}
}

impl From<VulkanMeshError> for AssetManagerError {
	fn from(value: VulkanMeshError) -> Self {
		Self::Mesh(value)
	}
}

impl AssetManager {
	/// Logical vertex/index/RGBA storage, excluding driver alignment, staging
	/// buffers and renderer-owned targets. Cached and external retained Arcs
	/// both count until their allocations are actually released.
	pub const GPU_LOGICAL_BYTE_LIMIT: usize = 512 * 1024 * 1024;
	pub const GPU_RESOURCE_LIMIT: usize = 8192;
	/// Import the first mesh and share already-uploaded GPU resources. Caches
	/// represent an asset snapshot until `clear`; no filesystem polling occurs.
	pub fn load_model(
		&mut self,
		renderer: &WindowRenderer,
		path: impl AsRef<Path>,
	) -> Result<Model, AssetManagerError> {
		self.load_model_mesh(renderer, path, 0)
			.map(|model| (*model).clone())
	}

	/// Select a mesh explicitly, retaining one Arc<Model> per asset and mesh.
	/// Scene nodes that reference the same mesh share all GPU allocations.
	pub fn load_model_mesh(
		&mut self,
		renderer: &WindowRenderer,
		path: impl AsRef<Path>,
		mesh_index: usize,
	) -> Result<Arc<Model>, AssetManagerError> {
		let full_path = self.resolve(path)?;
		let model_key = engine_core::asset_registry::AssetRef::new(
			full_path
				.strip_prefix(&self.root)
				.map_err(|_| {
					AssetManagerError::ResourceBudget(
						"Canonical model escaped the asset root".into(),
					)
				})?
				.to_string_lossy()
				.replace('\\', "/"),
			Some(mesh_index),
		)
		.map_err(|error| AssetManagerError::Gltf(GltfLoaderError::Resource(error)))?;
		if let Some(model) = self.models.get(&model_key) {
			return Ok(Arc::clone(model));
		}
		if self.models.len() >= Self::GPU_RESOURCE_LIMIT {
			return Err(AssetManagerError::ResourceBudget(
				"model cache entry limit reached; retire unused models first".into(),
			));
		}
		self.cache_resources(&full_path)?;
		let (resources, _) = &self.resource_cache[&full_path];
		let imported_parts = import_mesh(&resources.0, &resources.1, &resources.2, mesh_index)?;
		self.preflight_model_upload(&full_path, &imported_parts)?;
		self.upload_model_resources(renderer, &full_path, &imported_parts)?;
		let mut parts = Vec::with_capacity(imported_parts.len());
		let mut bounds: Option<crate::Bounds3> = None;

		for imported in imported_parts {
			let primitive_bounds = crate::Bounds3::from_points(
				imported
					.mesh
					.vertices()
					.iter()
					.map(|vertex| vertex.position),
			)
			.map_err(|error| AssetManagerError::Gltf(GltfLoaderError::Resource(error)))?;
			if let Some(primitive_bounds) = primitive_bounds {
				bounds =
					Some(bounds.map_or(primitive_bounds, |bounds| bounds.union(primitive_bounds)));
			}
			let mesh_key = format!(
				"{}#mesh:{}#primitive:{}",
				full_path.display(),
				imported.mesh_index,
				imported.primitive_index,
			);
			let image_key = match imported.base_color_image_index {
				Some(index) => format!("{}#image:{}", full_path.display(), index),
				None => "builtin:white".to_string(),
			};
			let mesh = self.load_mesh(renderer, mesh_key, &imported.mesh)?;
			let material = self.load_material(renderer, &image_key, &imported.material)?;
			engine_logger::info!(
				"Prepared primitive {}: {} indices, double-sided {}",
				imported.primitive_index,
				mesh.index_count(),
				material.double_sided(),
			);
			parts.push(ModelPart::new(mesh, material));
		}

		engine_logger::info!(
			"Loaded {}: {} model parts; GPU meshes: {}, GPU textures: {}",
			full_path.display(),
			parts.len(),
			self.loaded_mesh_count(),
			self.loaded_texture_count(),
		);
		let model = Arc::new(Model::with_bounds(parts, bounds));
		self.models.insert(model_key, Arc::clone(&model));
		Ok(model)
	}

	/// GPU factory boundary for renderer-independent logical asset references.
	/// The first mesh is normalized to index zero in the shared model cache.
	pub fn load_asset(
		&mut self,
		renderer: &WindowRenderer,
		reference: &engine_core::asset_registry::AssetRef,
	) -> Result<Arc<Model>, AssetManagerError> {
		reference
			.validate()
			.map_err(|error| AssetManagerError::Gltf(GltfLoaderError::Resource(error)))?;
		self.load_model_mesh(renderer, &reference.path, reference.mesh_index.unwrap_or(0))
	}

	/// CPU-only hierarchy metadata for the selected default glTF scene. This
	/// does not decode images, allocate GPU objects, or apply node transforms.
	pub fn load_scene(
		&mut self,
		path: impl AsRef<Path>,
	) -> Result<Arc<super::ImportedScene>, AssetManagerError> {
		let full_path = self.resolve(path)?;
		if let Some(scene) = self.scenes.get(&full_path) {
			return Ok(Arc::clone(scene));
		}
		let scene = Arc::new(gltf_loader::load_scene_in_root(&full_path, &self.root)?);
		const SCENE_CACHE_LIMIT: usize = 32 * 1024 * 1024;
		let bytes = scene.retained_bytes() + full_path.as_os_str().len() * 4 + 128;
		if bytes > SCENE_CACHE_LIMIT {
			return Ok(scene);
		}
		while self.scenes.len() >= 8 || self.scene_bytes + bytes > SCENE_CACHE_LIMIT {
			if let Some(oldest) = self.scene_order.pop_front()
				&& let Some(scene) = self.scenes.remove(&oldest)
			{
				self.scene_bytes -= scene.retained_bytes() + oldest.as_os_str().len() * 4 + 128;
			}
		}
		self.scenes.insert(full_path.clone(), Arc::clone(&scene));
		self.scene_order.push_back(full_path);
		self.scene_bytes += bytes;
		Ok(scene)
	}

	fn cache_resources(&mut self, path: &Path) -> Result<(), AssetManagerError> {
		if self.resource_cache.contains_key(path) {
			return Ok(());
		}
		let resources = load_resources_in_root(path, &self.root)?;
		let bytes = resources
			.1
			.iter()
			.map(|buffer| buffer.0.len())
			.chain(resources.2.iter().map(|image| image.pixels.len()))
			.try_fold(0usize, |total, bytes| total.checked_add(bytes))
			.ok_or_else(|| GltfLoaderError::Resource("CPU asset size overflow".into()))?;
		if bytes > gltf_loader::IMPORT_LIMIT {
			return Err(GltfLoaderError::Resource("CPU asset cache exceeds 256 MiB".into()).into());
		}
		while self.resource_cache.len() >= 4
			|| self.resource_bytes.saturating_add(bytes) > gltf_loader::IMPORT_LIMIT
		{
			if let Some(oldest) = self.resource_order.pop_front()
				&& let Some((_, bytes)) = self.resource_cache.remove(&oldest)
			{
				self.resource_bytes -= bytes;
			}
		}
		self.resource_order.push_back(path.to_path_buf());
		self.resource_bytes += bytes;
		self.resource_cache
			.insert(path.to_path_buf(), (resources, bytes));
		Ok(())
	}

	pub fn new(root: impl AsRef<Path>) -> Result<Self, AssetManagerError> {
		let root = root.as_ref().canonicalize()?;

		Ok(Self {
			root,
			textures: HashMap::new(),
			meshes: HashMap::new(),
			image_textures: HashMap::new(),
			models: HashMap::new(),
			scenes: HashMap::new(),
			scene_order: VecDeque::new(),
			scene_bytes: 0,
			resource_cache: HashMap::new(),
			resource_order: VecDeque::new(),
			resource_bytes: 0,
			gpu_allocations: Vec::new(),
			gpu_bytes: 0,
		})
	}

	/*
	 * Texture assets are identified by their
	 * canonical file path.
	 */
	pub fn load_texture(
		&mut self,

		renderer: &WindowRenderer,

		path: impl AsRef<Path>,
	) -> Result<Arc<VulkanTexture>, AssetManagerError> {
		let full_path = self.resolve(path)?;

		if let Some(texture) = self.textures.get(&full_path) {
			return Ok(Arc::clone(texture));
		}

		let image = ImageAsset::load(&full_path)?;

		let bytes = image.pixels().len();
		self.check_gpu_budget(bytes, 1)?;

		let texture = renderer.upload_rgba8(image.width(), image.height(), image.pixels())?;
		texture.set_debug_label(&full_path.to_string_lossy());

		let texture = Arc::new(texture);
		self.gpu_allocations.push(GpuAllocation {
			resource: GpuReference::Texture(Arc::downgrade(&texture)),
			bytes,
		});
		self.gpu_bytes += bytes;

		self.textures.insert(full_path, Arc::clone(&texture));

		Ok(texture)
	}

	/*
	 * Upload CPU MeshData once and cache
	 * the resulting GPU mesh.
	 *
	 * For procedural/builtin resources use
	 * stable IDs such as:
	 *
	 * builtin:cube
	 *
	 * Later glTF can use IDs such as:
	 *
	 * models/scene.glb#mesh:2
	 */
	pub fn load_mesh(
		&mut self,

		renderer: &WindowRenderer,

		id: impl Into<String>,

		data: &MeshData,
	) -> Result<Arc<VulkanMesh>, AssetManagerError> {
		let id = id.into();

		if let Some(mesh) = self.meshes.get(&id) {
			return Ok(Arc::clone(mesh));
		}

		let bytes = mesh_bytes(data)?;
		self.check_gpu_budget(bytes, 1)?;

		let mesh = renderer.upload_mesh(data.vertices(), data.indices())?;
		mesh.set_debug_label(&id);

		let mesh = Arc::new(mesh);
		self.gpu_allocations.push(GpuAllocation {
			resource: GpuReference::Mesh(Arc::downgrade(&mesh)),
			bytes,
		});
		self.gpu_bytes += bytes;

		self.meshes.insert(id, Arc::clone(&mesh));

		Ok(mesh)
	}

	pub fn loaded_texture_count(&self) -> usize {
		self.textures.len() + self.image_textures.len()
	}

	pub fn loaded_mesh_count(&self) -> usize {
		self.meshes.len()
	}

	pub fn gpu_logical_bytes(&self) -> usize {
		self.gpu_allocations
			.iter()
			.filter(|allocation| allocation.live())
			.map(|allocation| allocation.bytes)
			.sum()
	}

	pub fn gpu_resource_count(&self) -> usize {
		self.gpu_allocations
			.iter()
			.filter(|allocation| allocation.live())
			.count()
	}

	pub fn contains_texture(&self, path: impl AsRef<Path>) -> bool {
		let Ok(full_path) = self.resolve(path) else {
			return false;
		};

		self.textures.contains_key(&full_path)
	}

	pub fn contains_mesh(&self, id: &str) -> bool {
		self.meshes.contains_key(id)
	}

	pub fn clear(&mut self) {
		self.models.clear();
		self.scenes.clear();
		self.scene_order.clear();
		self.scene_bytes = 0;
		self.resource_cache.clear();
		self.resource_order.clear();
		self.resource_bytes = 0;
		self.meshes.clear();

		self.textures.clear();
		self.image_textures.clear();
		self.reap_gpu_allocations();
	}

	/// Retire cache-only GPU resources after the caller has waited for every
	/// submitted frame using them to finish. Active scenes and legacy Model
	/// clones retain their own resource Arcs; those allocations remain cached.
	/// Prune models first so their mesh/material references no longer keep
	/// otherwise-unused GPU allocations alive.
	pub fn prune_unused(&mut self) {
		self.models.retain(|_, model| Arc::strong_count(model) > 1);
		self.meshes.retain(|_, mesh| Arc::strong_count(mesh) > 1);
		self.textures
			.retain(|_, texture| Arc::strong_count(texture) > 1);
		self.image_textures
			.retain(|_, texture| Arc::strong_count(texture) > 1);
		self.reap_gpu_allocations();
	}

	pub fn load_image_texture(
		&mut self,
		renderer: &WindowRenderer,
		id: impl Into<String>,
		image: &ImageAsset,
	) -> Result<Arc<VulkanTexture>, AssetManagerError> {
		let id = id.into();

		if let Some(texture) = self.image_textures.get(&id) {
			return Ok(Arc::clone(texture));
		}

		let bytes = image.pixels().len();
		self.check_gpu_budget(bytes, 1)?;

		let texture =
			Arc::new(renderer.upload_rgba8(image.width(), image.height(), image.pixels())?);
		texture.set_debug_label(&id);
		self.gpu_allocations.push(GpuAllocation {
			resource: GpuReference::Texture(Arc::downgrade(&texture)),
			bytes,
		});
		self.gpu_bytes += bytes;

		self.image_textures.insert(id, Arc::clone(&texture));
		Ok(texture)
	}

	pub fn load_material(
		&mut self,
		renderer: &WindowRenderer,
		image_id: &str,
		data: &MaterialData,
	) -> Result<VulkanMaterial, AssetManagerError> {
		let texture = match data.base_color_image.as_ref() {
			Some(image) => self.load_image_texture(renderer, image_id, image)?,
			None => {
				if let Some(texture) = self.image_textures.get("builtin:white") {
					Arc::clone(texture)
				} else {
					let white = ImageAsset::from_rgba8(1, 1, vec![255, 255, 255, 255])?;
					self.load_image_texture(renderer, "builtin:white", &white)?
				}
			}
		};

		Ok(VulkanMaterial::new(
			texture,
			data.double_sided,
			data.base_color_factor,
		))
	}

	fn resolve(&self, path: impl AsRef<Path>) -> Result<PathBuf, AssetManagerError> {
		let path = self.root.join(path.as_ref()).canonicalize()?;

		if !path.starts_with(&self.root) {
			return Err(AssetManagerError::OutsideAssetRoot {
				path,

				root: self.root.clone(),
			});
		}

		Ok(path)
	}

	fn reap_gpu_allocations(&mut self) {
		self.gpu_allocations.retain(GpuAllocation::live);
		self.gpu_bytes = self
			.gpu_allocations
			.iter()
			.map(|allocation| allocation.bytes)
			.sum();
	}

	fn check_gpu_budget(&mut self, bytes: usize, entries: usize) -> Result<(), AssetManagerError> {
		// Cached totals are conservative when callers have dropped their last Arc.
		// Most uploads fit without traversing every previously loaded resource.
		if self.gpu_budget_fits(bytes, entries) {
			return Ok(());
		}
		self.reap_gpu_allocations();
		if !self.gpu_budget_fits(bytes, entries) {
			return Err(AssetManagerError::ResourceBudget(format!(
				"request needs {bytes} logical bytes and {entries} resources; {} bytes/{} resources remain live (limits: {} bytes and {}). Release unused scene resources and prune the cache before retrying",
				self.gpu_bytes,
				self.gpu_allocations.len(),
				Self::GPU_LOGICAL_BYTE_LIMIT,
				Self::GPU_RESOURCE_LIMIT
			)));
		}
		Ok(())
	}

	fn gpu_budget_fits(&self, bytes: usize, entries: usize) -> bool {
		self.gpu_bytes
			.checked_add(bytes)
			.is_some_and(|total| total <= Self::GPU_LOGICAL_BYTE_LIMIT)
			&& self
				.gpu_allocations
				.len()
				.checked_add(entries)
				.is_some_and(|count| count <= Self::GPU_RESOURCE_LIMIT)
	}

	fn preflight_model_upload(
		&mut self,
		path: &Path,
		parts: &[ImportedPrimitive],
	) -> Result<(), AssetManagerError> {
		let mut bytes = 0usize;
		let mut entries = 0usize;
		let mut images = HashSet::new();
		for part in parts {
			let mesh_key = format!(
				"{}#mesh:{}#primitive:{}",
				path.display(),
				part.mesh_index,
				part.primitive_index
			);
			if !self.meshes.contains_key(&mesh_key) {
				bytes = bytes.checked_add(mesh_bytes(&part.mesh)?).ok_or_else(|| {
					AssetManagerError::ResourceBudget("mesh byte count overflow".into())
				})?;
				entries += 1;
			}
			let image_key = part.base_color_image_index.map_or_else(
				|| "builtin:white".to_owned(),
				|index| format!("{}#image:{index}", path.display()),
			);
			if !self.image_textures.contains_key(&image_key) && images.insert(image_key) {
				bytes = bytes
					.checked_add(
						part.material
							.base_color_image
							.as_ref()
							.map_or(4, |image| image.pixels().len()),
					)
					.ok_or_else(|| {
						AssetManagerError::ResourceBudget("texture byte count overflow".into())
					})?;
				entries += 1;
			}
		}
		self.check_gpu_budget(bytes, entries)
	}

	fn upload_model_resources(
		&mut self,
		renderer: &WindowRenderer,
		path: &Path,
		parts: &[ImportedPrimitive],
	) -> Result<(), AssetManagerError> {
		let white = ImageAsset::from_rgba8(1, 1, vec![255; 4])?;
		let mut pending = Vec::new();
		let mut images = HashSet::new();
		for part in parts {
			let mesh_id = format!(
				"{}#mesh:{}#primitive:{}",
				path.display(),
				part.mesh_index,
				part.primitive_index
			);
			if !self.meshes.contains_key(&mesh_id) {
				pending.push(PendingUpload::Mesh {
					id: mesh_id,
					data: &part.mesh,
					bytes: mesh_bytes(&part.mesh)?,
				});
			}
			let image_id = part.base_color_image_index.map_or_else(
				|| "builtin:white".to_owned(),
				|index| format!("{}#image:{index}", path.display()),
			);
			if !self.image_textures.contains_key(&image_id) && images.insert(image_id.clone()) {
				pending.push(PendingUpload::Texture {
					id: image_id,
					image: part.material.base_color_image.as_ref().unwrap_or(&white),
				});
			}
		}
		let mut remaining = pending.as_slice();
		while !remaining.is_empty() {
			let count = next_upload_batch_len(remaining.iter().map(PendingUpload::bytes));
			let (batch, tail) = remaining.split_at(count);
			if batch[0].bytes() > renderer::MAX_UPLOAD_BATCH_BYTES {
				// A single asset retains the established larger upload allowance.
				match &batch[0] {
					PendingUpload::Mesh { id, data, .. } => {
						self.load_mesh(renderer, id.clone(), data)?;
					}
					PendingUpload::Texture { id, image } => {
						self.load_image_texture(renderer, id.clone(), image)?;
					}
				}
			} else {
				let meshes: Vec<_> = batch
					.iter()
					.filter_map(|upload| match upload {
						PendingUpload::Mesh { data, .. } => Some(renderer::MeshUpload {
							vertices: data.vertices(),
							indices: data.indices(),
						}),
						_ => None,
					})
					.collect();
				let textures: Vec<_> = batch
					.iter()
					.filter_map(|upload| match upload {
						PendingUpload::Texture { image, .. } => Some(renderer::Rgba8Upload {
							width: image.width(),
							height: image.height(),
							pixels: image.pixels(),
						}),
						_ => None,
					})
					.collect();
				let uploaded = renderer
					.upload_batch(&meshes, &textures)
					.map_err(AssetManagerError::Upload)?;
				let mut meshes = uploaded.meshes.into_iter();
				let mut textures = uploaded.textures.into_iter();
				// Upload completion owns all staging/fence handling; only completed
				// resources become visible in caches. Earlier chunks stay reusable if
				// a later chunk fails, matching individual-upload cache semantics.
				for upload in batch {
					let bytes = upload.bytes();
					match upload {
						PendingUpload::Mesh { id, .. } => {
							let mesh = Arc::new(meshes.next().expect("one mesh per upload"));
							mesh.set_debug_label(id);
							self.gpu_allocations.push(GpuAllocation {
								resource: GpuReference::Mesh(Arc::downgrade(&mesh)),
								bytes,
							});
							self.meshes.insert(id.clone(), mesh);
						}
						PendingUpload::Texture { id, .. } => {
							let texture =
								Arc::new(textures.next().expect("one texture per upload"));
							texture.set_debug_label(id);
							self.gpu_allocations.push(GpuAllocation {
								resource: GpuReference::Texture(Arc::downgrade(&texture)),
								bytes,
							});
							self.image_textures.insert(id.clone(), texture);
						}
					}
					self.gpu_bytes += bytes;
				}
			}
			remaining = tail;
		}
		Ok(())
	}
}

enum PendingUpload<'a> {
	Mesh {
		id: String,
		data: &'a MeshData,
		bytes: usize,
	},
	Texture {
		id: String,
		image: &'a ImageAsset,
	},
}

impl PendingUpload<'_> {
	fn bytes(&self) -> usize {
		match self {
			Self::Mesh { bytes, .. } => *bytes,
			Self::Texture { image, .. } => image.pixels().len(),
		}
	}
}

fn next_upload_batch_len(sizes: impl Iterator<Item = usize>) -> usize {
	let mut bytes = 0usize;
	let mut count = 0;
	for size in sizes.take(renderer::MAX_UPLOAD_BATCH_RESOURCES) {
		if size > renderer::MAX_UPLOAD_BATCH_BYTES {
			return count.max(1);
		}
		if bytes > renderer::MAX_UPLOAD_BATCH_BYTES - size {
			break;
		}
		bytes += size;
		count += 1;
	}
	count
}

fn mesh_bytes(data: &MeshData) -> Result<usize, AssetManagerError> {
	data.vertices()
		.len()
		.checked_mul(std::mem::size_of::<Vertex>())
		.and_then(|vertices| {
			data.indices()
				.len()
				.checked_mul(std::mem::size_of::<u32>())
				.and_then(|indices| vertices.checked_add(indices))
		})
		.ok_or_else(|| AssetManagerError::ResourceBudget("mesh byte count overflow".into()))
}

#[cfg(test)]
mod accounting_tests {
	use super::*;
	#[test]
	fn upload_chunks_bound_staging_and_resource_counts_and_isolate_large_assets() {
		let limit = renderer::MAX_UPLOAD_BATCH_BYTES;
		assert_eq!(
			next_upload_batch_len([limit / 2, limit / 2, 4].into_iter()),
			2
		);
		assert_eq!(next_upload_batch_len([4, limit + 1].into_iter()), 1);
		assert_eq!(next_upload_batch_len([limit + 1, 4].into_iter()), 1);
		assert_eq!(
			next_upload_batch_len(std::iter::repeat(4)),
			renderer::MAX_UPLOAD_BATCH_RESOURCES
		);
		assert_eq!(next_upload_batch_len([].into_iter()), 0);
	}
	#[test]
	fn conservative_budget_skips_sweeps_until_pressure_and_rejects_oversized_requests() {
		let mut assets = AssetManager::new(env!("CARGO_MANIFEST_DIR")).unwrap();
		assets.gpu_allocations.push(GpuAllocation {
			resource: GpuReference::Mesh(Weak::new()),
			bytes: 4,
		});
		assets.gpu_bytes = 4;
		assets.check_gpu_budget(4, 1).unwrap();
		assert_eq!(
			assets.gpu_allocations.len(),
			1,
			"ordinary upload must not sweep allocations"
		);
		assets
			.check_gpu_budget(AssetManager::GPU_LOGICAL_BYTE_LIMIT, 1)
			.unwrap();
		assert!(assets.gpu_allocations.is_empty());
		assert_eq!(assets.gpu_bytes, 0);
		assert!(assets.check_gpu_budget(usize::MAX, 1).is_err());
		assert!(
			assets
				.check_gpu_budget(0, AssetManager::GPU_RESOURCE_LIMIT + 1)
				.is_err()
		);
	}
}
