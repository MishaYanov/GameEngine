use std::{
	collections::{HashMap, HashSet},
	error::Error,
	fmt::{Display, Formatter},
	io,
	path::{Path, PathBuf},
	sync::Arc,
};

use renderer::{
	WindowRenderer,
	vulkan::{
		Vertex, VulkanMaterial, VulkanMesh, VulkanMeshError, VulkanTexture, VulkanTextureError,
	},
};

use super::{
	ImageAsset, ImageAssetError, MaterialData, MeshData, Model, PreparedModelBatch,
	gltf_loader::{GltfLoaderError, ImportedPrimitive},
	model::ModelPart,
	warm_model::CurrentModel,
};

mod gpu_budget;
mod sky;
mod source_cache;
mod upload;

use gpu_budget::GpuBudget;
use source_cache::SourceCache;
use upload::mesh_bytes;

pub struct AssetManager {
	root: PathBuf,
	textures: HashMap<PathBuf, Arc<VulkanTexture>>,
	meshes: HashMap<String, Arc<VulkanMesh>>,
	image_textures: HashMap<String, Arc<VulkanTexture>>,
	models: HashMap<engine_core::asset_registry::AssetRef, Arc<Model>>,
	animated_models: HashMap<String, Arc<Model>>,
	source_fingerprints: HashMap<PathBuf, String>,
	source_cache: SourceCache,
	gpu_budget: GpuBudget,
	sky_texture: Option<sky::SkyTextureSlot>,
	sky_decode: Option<sky::SkyDecode>,
	sky_update_deadline: Option<std::time::Instant>,
}

impl AssetManager {
	pub const GPU_LOGICAL_BYTE_LIMIT: usize = GpuBudget::MAX_BYTES;
	pub const GPU_RESOURCE_LIMIT: usize = GpuBudget::MAX_RESOURCES;

	pub fn new(root: impl AsRef<Path>) -> Result<Self, AssetManagerError> {
		let root = root.as_ref().canonicalize()?;

		Ok(Self {
			root,
			textures: HashMap::new(),
			meshes: HashMap::new(),
			image_textures: HashMap::new(),
			models: HashMap::new(),
			animated_models: HashMap::new(),
			source_fingerprints: HashMap::new(),
			source_cache: SourceCache::default(),
			gpu_budget: GpuBudget::default(),
			sky_texture: None,
			sky_decode: None,
			sky_update_deadline: None,
		})
	}

	pub fn root(&self) -> &Path {
		&self.root
	}
	pub fn cached_animation(
		&self,
		source: &engine_core::animation::AnimationSourceRef,
	) -> Option<Arc<engine_core::animation::AnimationAsset>> {
		let key = format!(
			"{}:{}:{}",
			source.expected_sha256, source.mesh_node, source.skin_index
		);
		self.animated_models
			.get(&key)
			.and_then(|m| m.animation_asset())
			.filter(|a| a.source_scene == source.scene_index)
			.cloned()
	}
	pub fn load_animation(
		&mut self,
		renderer: &WindowRenderer,
		asset: Arc<engine_core::animation::AnimationAsset>,
	) -> Result<Arc<Model>, AssetManagerError> {
		let key = format!(
			"{}:{}:{}",
			asset.source_sha256, asset.skin.source_mesh_node, asset.skin.source_skin
		);
		if let Some(model) = self.animated_models.get(&key) {
			return Ok(model.clone());
		}
		if self.animated_models.len() >= 64 {
			return Err(AssetManagerError::ResourceBudget(
				"Animated model cache limit reached".into(),
			));
		}
		let bytes: usize = asset
			.primitives
			.iter()
			.map(|p| {
				p.vertices.len()
					* (size_of::<Vertex>() + size_of::<renderer::vulkan::mesh::SkinInfluence>())
					+ p.indices.len() * 4
			})
			.sum();
		self.gpu_budget.admit(
			bytes + asset.images.iter().map(|i| i.rgba.len()).sum::<usize>() + 4,
			asset.primitives.len() + asset.images.len() + 1,
		)?;
		// Construct/copy each decoded image only on its first cache miss, even
		// when hundreds of primitives share the same material image.
		let mut textures = Vec::with_capacity(asset.images.len());
		for (index, image) in asset.images.iter().enumerate() {
			let image_id = format!("animation:{}:{index}", asset.source_sha256);
			let texture = if let Some(texture) = self.image_textures.get(&image_id) {
				texture.clone()
			} else {
				let image = ImageAsset::from_rgba8(image.width, image.height, image.rgba.clone())?;
				self.load_image_texture(renderer, image_id, &image)?
			};
			textures.push(texture);
		}
		let white = if asset
			.primitives
			.iter()
			.any(|p| p.base_color_image.is_none())
		{
			Some(
				if let Some(texture) = self.image_textures.get("builtin:white") {
					texture.clone()
				} else {
					self.load_image_texture(
						renderer,
						"builtin:white",
						&ImageAsset::from_rgba8(1, 1, vec![255; 4])?,
					)?
				},
			)
		} else {
			None
		};
		let mut parts = Vec::new();
		for primitive in &asset.primitives {
			let vertices: Vec<_> = primitive
				.vertices
				.iter()
				.map(|v| Vertex {
					position: v.position,
					normal: v.normal,
					uv: v.uv,
				})
				.collect();
			let influences: Vec<_> = primitive
				.vertices
				.iter()
				.map(|v| renderer::vulkan::mesh::SkinInfluence {
					joints: v.joints,
					weights: v.weights,
				})
				.collect();
			let mesh = Arc::new(renderer.upload_skinned_mesh(
				&vertices,
				&primitive.indices,
				&influences,
			)?);
			let logical_bytes = vertices.len()
				* (size_of::<Vertex>() + size_of::<renderer::vulkan::mesh::SkinInfluence>())
				+ primitive.indices.len() * 4;
			self.gpu_budget.track_mesh(&mesh, logical_bytes);
			let texture = match primitive.base_color_image {
				Some(index) => textures[index].clone(),
				None => white
					.as_ref()
					.expect("untextured primitive preflight")
					.clone(),
			};
			let material = VulkanMaterial::new(texture, primitive.double_sided, primitive.color);
			parts.push(ModelPart::new(mesh, material));
		}
		let model = Arc::new(Model::skinned(parts, asset));
		self.animated_models.insert(key, model.clone());
		Ok(model)
	}

	/// Logical vertex/index/RGBA storage, excluding driver alignment, staging
	/// buffers and renderer-owned targets. Cached and external retained Arcs
	/// both count until their allocations are actually released.
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

	/// Select a mesh explicitly, retaining one `Arc<Model>` per asset and mesh.
	/// Scene nodes that reference the same mesh share all GPU allocations.
	pub fn load_model_mesh(
		&mut self,
		renderer: &WindowRenderer,
		path: impl AsRef<Path>,
		mesh_index: usize,
	) -> Result<Arc<Model>, AssetManagerError> {
		self.load_model_mesh_prepared(renderer, path.as_ref(), mesh_index, None)
	}

	fn load_model_mesh_prepared(
		&mut self,
		renderer: &WindowRenderer,
		path: &Path,
		mesh_index: usize,
		warming: Option<(
			&mut PreparedModelBatch,
			&engine_core::asset_registry::AssetRef,
			CurrentModel,
		)>,
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
		let (source_fingerprint, imported_parts) = if let Some((fingerprint, parts)) = warming
			.and_then(|(batch, reference, source)| {
				batch.take_current(&self.root, reference, &source, &full_path)
			}) {
			(fingerprint, parts)
		} else {
			let resources = self
				.source_cache
				.prepare_resources(&self.root, &full_path, mesh_index)?;
			(
				resources.source_fingerprint.clone(),
				resources.import_mesh(mesh_index)?,
			)
		};
		self.upload_model(
			renderer,
			full_path,
			model_key,
			imported_parts,
			source_fingerprint,
		)
	}

	fn upload_model(
		&mut self,
		renderer: &WindowRenderer,
		full_path: PathBuf,
		model_key: engine_core::asset_registry::AssetRef,
		imported_parts: Vec<ImportedPrimitive>,
		source_fingerprint: String,
	) -> Result<Arc<Model>, AssetManagerError> {
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
			engine_logger::debug!(
				"Prepared primitive {}: {} indices, double-sided {}",
				imported.primitive_index,
				mesh.index_count(),
				material.double_sided(),
			);
			parts.push(ModelPart::new(mesh, material));
		}

		engine_logger::debug!(
			"Model uploaded: {} model parts; GPU meshes: {}, GPU textures: {}",
			parts.len(),
			self.loaded_mesh_count(),
			self.loaded_texture_count(),
		);
		let model = Arc::new(Model::with_bounds(parts, bounds));
		self.models.insert(model_key, Arc::clone(&model));
		self.source_fingerprints
			.insert(full_path, source_fingerprint);
		Ok(model)
	}

	/// GPU factory boundary for renderer-independent logical asset references.
	/// The first mesh is normalized to index zero in the shared model cache.
	pub fn load_asset(
		&mut self,
		renderer: &WindowRenderer,
		reference: &engine_core::asset_registry::AssetRef,
	) -> Result<Arc<Model>, AssetManagerError> {
		self.load_asset_prepared(renderer, reference, None)
	}

	pub(crate) fn load_asset_prepared(
		&mut self,
		renderer: &WindowRenderer,
		reference: &engine_core::asset_registry::AssetRef,
		warming: Option<&mut PreparedModelBatch>,
	) -> Result<Arc<Model>, AssetManagerError> {
		let (reference, warming) =
			super::project_assets::resolve_asset_with_model(&self.root, reference, |model| {
				warming
					.zip(model)
					.map(|(batch, asset)| (batch, CurrentModel::from(asset)))
			})
			.map_err(|error| AssetManagerError::Gltf(GltfLoaderError::Resource(error)))?;
		self.load_model_mesh_prepared(
			renderer,
			Path::new(&reference.path),
			reference.mesh_index.unwrap_or(0),
			warming.map(|(batch, source)| (batch, &reference, source)),
		)
	}

	/// Hidden authoring markers may deliberately have no model. Existing glTF
	/// models are prepared so a later visibility action needs no asset I/O. Test
	/// the resolved catalog path; aliases may retain an older file extension.
	pub fn asset_exists(
		&self,
		reference: &engine_core::asset_registry::AssetRef,
	) -> Result<bool, AssetManagerError> {
		let reference = super::project_assets::resolve_asset(&self.root, reference)
			.map_err(|error| AssetManagerError::Gltf(GltfLoaderError::Resource(error)))?;
		if !Path::new(&reference.path)
			.extension()
			.and_then(|extension| extension.to_str())
			.is_some_and(|extension| {
				extension.eq_ignore_ascii_case("gltf") || extension.eq_ignore_ascii_case("glb")
			}) {
			return Ok(false);
		}
		match self.resolve(&reference.path) {
			Ok(path) => Ok(path.is_file()),
			Err(AssetManagerError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
				Ok(false)
			}
			Err(error) => Err(error),
		}
	}

	/// Upload every compatible mesh without replacing an existing source key.
	/// Old scene resources and candidates share the same live-allocation budget.
	pub fn prepare_gpu_asset(
		&mut self,
		renderer: &WindowRenderer,
		change: &super::project_assets::PreparedAssetChange,
	) -> Result<PreparedGpuAsset, AssetManagerError> {
		let source = (!change.is_reimport())
			.then(|| change.asset().aliases.last())
			.flatten()
			.and_then(|alias| self.resolve(alias).ok())
			.filter(|path| self.source_fingerprints.get(path) == Some(&change.asset().fingerprint));
		let reuses_source = source.is_some();
		let candidate = match source {
			Some(path) => path,
			None => self.resolve(change.candidate_path())?,
		};
		let destination =
			super::project_assets::asset_path(&self.root, change.destination_path(), false)
				.map_err(|error| AssetManagerError::Gltf(GltfLoaderError::Resource(error)))?;
		let candidate_relative = candidate
			.strip_prefix(&self.root)
			.expect("resolved candidate belongs to asset root")
			.to_string_lossy()
			.replace('\\', "/");
		let mut prepared = PreparedGpuAsset {
			candidate,
			destination,
			candidate_relative,
			destination_relative: change.destination_path().to_owned(),
			reuses_source,
			animation_candidate: None,
		};
		if let Ok(asset) =
			bridge::animation_asset::load_path(&self.root, &prepared.candidate_relative)
		{
			let key = format!(
				"{}:{}:{}",
				asset.source_sha256, asset.skin.source_mesh_node, asset.skin.source_skin
			);
			if !self.animated_models.contains_key(&key) {
				prepared.animation_candidate = Some(key);
			}
			if let Err(error) = self.load_animation(renderer, asset) {
				self.discard_gpu_asset(prepared);
				return Err(error);
			}
			self.source_fingerprints.insert(
				prepared.candidate.clone(),
				change.asset().fingerprint.clone(),
			);
			return Ok(prepared);
		}
		for mesh in &change.asset().meshes {
			if let Err(error) = self.load_model_mesh(renderer, &prepared.candidate, mesh.index) {
				self.discard_gpu_asset(prepared);
				return Err(error);
			}
		}
		Ok(prepared)
	}

	/// Caller must complete submitted frames before retiring source cache owners.
	/// Existing external Model/Scene Arcs continue owning the old resources.
	pub fn promote_gpu_asset(&mut self, prepared: PreparedGpuAsset) {
		self.forget_source(&prepared.destination, &prepared.destination_relative);
		let models: Vec<_> = self
			.models
			.keys()
			.filter(|key| key.path == prepared.candidate_relative)
			.cloned()
			.collect();
		for key in models {
			if let Some(model) = self.models.remove(&key) {
				let mut destination = key;
				destination.path = prepared.destination_relative.clone();
				self.models.insert(destination, model);
			}
		}
		let from = prepared.candidate.to_string_lossy();
		let to = prepared.destination.to_string_lossy();
		remap_keys(&mut self.meshes, &from, &to);
		remap_keys(&mut self.image_textures, &from, &to);
		self.source_cache
			.remap(&prepared.candidate, &prepared.destination);
		if let Some(value) = self.source_fingerprints.remove(&prepared.candidate) {
			self.source_fingerprints
				.insert(prepared.destination.clone(), value);
		}
		if let Some(value) = self.textures.remove(&prepared.candidate) {
			self.textures.insert(prepared.destination, value);
		}
		self.gpu_budget.sweep();
	}

	pub fn discard_gpu_asset(&mut self, prepared: PreparedGpuAsset) {
		if let Some(key) = &prepared.animation_candidate {
			self.animated_models.remove(key);
			let hash = key.split(':').next().expect("animation cache key");
			let prefix = format!("animation:{hash}:");
			self.image_textures.retain(|id, _| !id.starts_with(&prefix));
		}
		if !prepared.reuses_source {
			self.forget_source(&prepared.candidate, &prepared.candidate_relative);
		}
		self.gpu_budget.sweep();
	}

	fn forget_source(&mut self, path: &Path, relative: &str) {
		self.models.retain(|key, _| key.path != relative);
		self.source_fingerprints.remove(path);
		let prefix = format!("{}#", path.display());
		self.meshes.retain(|key, _| !key.starts_with(&prefix));
		self.image_textures
			.retain(|key, _| !key.starts_with(&prefix));
		self.textures.remove(path);
		self.source_cache.remove(path);
	}

	/// CPU-only hierarchy metadata for the selected default glTF scene. This
	/// does not decode images, allocate GPU objects, or apply node transforms.
	pub fn load_scene(
		&mut self,
		path: impl AsRef<Path>,
	) -> Result<Arc<super::ImportedScene>, AssetManagerError> {
		let full_path = self.resolve(path)?;
		self.source_cache.load_scene(&self.root, full_path)
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
		self.gpu_budget.admit(bytes, 1)?;

		let texture = renderer.upload_rgba8(image.width(), image.height(), image.pixels())?;
		texture.set_debug_label(&full_path.to_string_lossy());

		let texture = Arc::new(texture);
		self.gpu_budget.track_texture(&texture, bytes);

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
		self.gpu_budget.admit(bytes, 1)?;

		let mesh = renderer.upload_mesh(data.vertices(), data.indices())?;
		mesh.set_debug_label(&id);

		let mesh = Arc::new(mesh);
		self.gpu_budget.track_mesh(&mesh, bytes);

		self.meshes.insert(id, Arc::clone(&mesh));

		Ok(mesh)
	}

	pub fn loaded_texture_count(&self) -> usize {
		self.textures.len() + self.image_textures.len()
	}

	pub fn loaded_mesh_count(&self) -> usize {
		self.meshes.len()
			+ self
				.animated_models
				.values()
				.map(|m| m.parts().len())
				.sum::<usize>()
	}
	pub fn loaded_model_count(&self) -> usize {
		self.models.len() + self.animated_models.len()
	}

	pub fn gpu_logical_bytes(&self) -> usize {
		self.gpu_budget.live_bytes()
	}

	pub fn gpu_resource_count(&self) -> usize {
		self.gpu_budget.live_resources()
	}

	/// Hosts may tighten the default upload budgets. Existing external scene Arcs
	/// stay charged; this never evicts or replaces them to admit a candidate.
	pub fn set_gpu_budget(
		&mut self,
		bytes: usize,
		resources: usize,
	) -> Result<(), AssetManagerError> {
		self.gpu_budget.set_limits(bytes, resources)
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
		self.animated_models.clear();
		self.source_fingerprints.clear();
		self.source_cache.clear();
		self.meshes.clear();

		self.textures.clear();
		self.image_textures.clear();
		self.gpu_budget.sweep();
	}

	/// Retire cache-only GPU resources after the caller has waited for every
	/// submitted frame using them to finish. Active scenes and legacy Model
	/// clones retain their own resource Arcs; those allocations remain cached.
	/// Prune models first so their mesh/material references no longer keep
	/// otherwise-unused GPU allocations alive. CPU source snapshots are retained
	/// only for live model paths or externally retained hierarchy metadata.
	pub fn prune_unused(&mut self) {
		self.animated_models
			.retain(|_, model| Arc::strong_count(model) > 1);
		self.models.retain(|_, model| {
			// A legacy owned Model clone retains meshes rather than Arc<Model>.
			// The cache and this model account for the first two mesh owners.
			Arc::strong_count(model) > 1
				|| model
					.parts()
					.iter()
					.any(|part| Arc::strong_count(part.mesh_arc()) > 2)
		});
		let retained_paths: HashSet<_> = self
			.models
			.keys()
			.map(|reference| self.root.join(&reference.path))
			.collect();
		self.source_fingerprints
			.retain(|path, _| retained_paths.contains(path));
		self.meshes.retain(|_, mesh| Arc::strong_count(mesh) > 1);
		self.textures
			.retain(|_, texture| Arc::strong_count(texture) > 1);
		self.image_textures
			.retain(|_, texture| Arc::strong_count(texture) > 1);
		self.gpu_budget.sweep();
		self.source_cache.prune_unused(&retained_paths);
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
		self.gpu_budget.admit(bytes, 1)?;

		let texture =
			Arc::new(renderer.upload_rgba8(image.width(), image.height(), image.pixels())?);
		texture.set_debug_label(&id);
		self.gpu_budget.track_texture(&texture, bytes);

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
}

/// Uploaded resources remain private under the temporary source key until the
/// caller publishes the matching CPU asset. Promotion performs no I/O or GPU
/// allocation. Discard candidates on a rejected/stale publication.
pub struct PreparedGpuAsset {
	candidate: PathBuf,
	destination: PathBuf,
	candidate_relative: String,
	destination_relative: String,
	reuses_source: bool,
	animation_candidate: Option<String>,
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

fn remap_keys<T>(cache: &mut HashMap<String, T>, from: &str, to: &str) {
	let prefix = format!("{from}#");
	let keys: Vec<_> = cache
		.keys()
		.filter(|key| key.starts_with(&prefix))
		.cloned()
		.collect();
	for key in keys {
		if let Some(value) = cache.remove(&key) {
			cache.insert(format!("{to}{}", &key[from.len()..]), value);
		}
	}
}
