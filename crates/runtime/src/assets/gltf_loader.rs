use std::{
	collections::HashSet,
	error::Error,
	fmt::{self, Display, Formatter},
	path::Path,
};

use gltf::mesh::Mode;
use renderer::vulkan::Vertex;

use super::{ImageAsset, ImageAssetError, MaterialData, MeshData};

#[cfg(test)]
#[path = "tests_gltf_loader.rs"]
mod tests;

pub struct ImportedPrimitive {
	pub mesh: MeshData,
	pub material: MaterialData,
	pub primitive_index: usize,
	pub mesh_index: usize,
	pub base_color_image_index: Option<usize>,
}

pub use super::scene_import::{ImportedNode, ImportedScene};

/// Read the default scene (or the first scene when no default is declared).
/// Geometry stays mesh-local; node transforms are returned separately.
pub fn load_scene(path: impl AsRef<Path>) -> Result<ImportedScene, GltfLoaderError> {
	let path = path.as_ref().canonicalize().map_err(rejected)?;
	let root = path
		.parent()
		.ok_or_else(|| rejected("missing model directory"))?;
	load_scene_in_root(&path, root)
}

pub(super) fn load_scene_in_root(
	path: &Path,
	root: &Path,
) -> Result<ImportedScene, GltfLoaderError> {
	super::metadata_import::load_scene_metadata(path, root, None, None)
}

#[derive(Debug)]
pub enum GltfLoaderError {
	Resource(String),
	Import(gltf::Error),
	Image(ImageAssetError),
	NoMeshes,
	NoPrimitives,
	UnsupportedMode(Mode),
	MissingPositions,
	MissingIndices,
	MissingTexCoords(u32),
	EmptyGeometry,
	TexCoordCountMismatch { positions: usize, tex_coords: usize },
	InvalidTriangleIndexCount(usize),
	IndexOutOfBounds { index: u32, vertex_count: usize },
	InvalidImageReference(usize),
	InvalidRgbByteCount(usize),
	UnsupportedImageFormat(gltf::image::Format),
}

impl Display for GltfLoaderError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
		match self {
			Self::Resource(message) => write!(formatter, "glTF resource rejected: {message}"),
			Self::Import(error) => {
				write!(formatter, "glTF import failed: {error}")
			}
			Self::Image(error) => {
				write!(formatter, "glTF image conversion failed: {error}")
			}
			Self::NoMeshes => {
				write!(formatter, "glTF document contains no meshes")
			}
			Self::NoPrimitives => {
				write!(formatter, "selected mesh contains no primitives")
			}
			Self::UnsupportedMode(mode) => {
				write!(formatter, "unsupported primitive mode: {mode:?}")
			}
			Self::MissingPositions => {
				write!(formatter, "primitive has no readable positions")
			}
			Self::MissingIndices => {
				write!(formatter, "primitive has no readable indices")
			}
			Self::MissingTexCoords(set) => {
				write!(
					formatter,
					"material requires missing texture-coordinate set {set}"
				)
			}
			Self::EmptyGeometry => {
				write!(formatter, "primitive contains empty geometry")
			}
			Self::TexCoordCountMismatch {
				positions,
				tex_coords,
			} => {
				write!(
					formatter,
					"position count {positions} differs from UV count {tex_coords}"
				)
			}
			Self::InvalidTriangleIndexCount(count) => {
				write!(
					formatter,
					"triangle index count {count} is not divisible by three"
				)
			}
			Self::IndexOutOfBounds {
				index,
				vertex_count,
			} => {
				write!(
					formatter,
					"index {index} is outside the mesh's {vertex_count} vertices"
				)
			}
			Self::InvalidImageReference(index) => {
				write!(formatter, "invalid image reference: {index}")
			}
			Self::InvalidRgbByteCount(bytes) => {
				write!(
					formatter,
					"invalid RGB byte count: {bytes} (must be divisible by three)"
				)
			}
			Self::UnsupportedImageFormat(format) => {
				write!(formatter, "unsupported image format: {format:?}")
			}
		}
	}
}

impl Error for GltfLoaderError {
	fn source(&self) -> Option<&(dyn Error + 'static)> {
		match self {
			Self::Import(error) => Some(error),
			Self::Image(error) => Some(error),
			_ => None,
		}
	}
}

impl From<gltf::Error> for GltfLoaderError {
	fn from(error: gltf::Error) -> Self {
		Self::Import(error)
	}
}

impl From<ImageAssetError> for GltfLoaderError {
	fn from(error: ImageAssetError) -> Self {
		Self::Image(error)
	}
}

/// Import every primitive of the first mesh. Scene-node transforms are not
/// applied yet; all returned geometry remains in mesh-local coordinates.
pub fn load_first_mesh(path: impl AsRef<Path>) -> Result<Vec<ImportedPrimitive>, GltfLoaderError> {
	let path = path.as_ref();
	let mut resources = load_resources(path)?;
	if resources.document.meshes().next().is_none() {
		return Err(GltfLoaderError::NoMeshes);
	}
	resources.prepare_mesh_images(0)?;
	let parts = import_first_mesh(&resources.document, &resources.buffers, &resources.images)?;
	engine_logger::debug!("Imported glTF mesh: parts={}", parts.len());
	Ok(parts)
}

const RESOURCE_LIMIT: usize = 64 * 1024 * 1024;
pub(super) const IMPORT_LIMIT: usize = 256 * 1024 * 1024;

fn rejected(message: impl ToString) -> GltfLoaderError {
	GltfLoaderError::Resource(message.to_string())
}

fn charge(budget: &mut usize, bytes: usize) -> Result<(), GltfLoaderError> {
	*budget = budget
		.checked_sub(bytes)
		.ok_or_else(|| rejected("import exceeds 256 MiB"))?;
	Ok(())
}

fn read_bounded(path: &Path, budget: &mut usize) -> Result<Vec<u8>, GltfLoaderError> {
	use std::io::Read;
	let file = std::fs::File::open(path).map_err(rejected)?;
	let metadata = file.metadata().map_err(rejected)?;
	if !metadata.is_file() || metadata.len() > RESOURCE_LIMIT.min(*budget) as u64 {
		return Err(rejected(
			"resource is not a regular file or exceeds byte limit",
		));
	}
	let mut bytes = Vec::new();
	file.take(RESOURCE_LIMIT.min(*budget) as u64 + 1)
		.read_to_end(&mut bytes)
		.map_err(rejected)?;
	if bytes.len() > RESOURCE_LIMIT {
		return Err(rejected("resource exceeds 64 MiB"));
	}
	charge(budget, bytes.len())?;
	Ok(bytes)
}

fn read_uri_in_root(
	base: &Path,
	root: &Path,
	uri: &str,
	budget: &mut usize,
) -> Result<Vec<u8>, GltfLoaderError> {
	if let Some(data) = uri.strip_prefix("data:") {
		let (_, encoded) = data
			.split_once(";base64,")
			.ok_or_else(|| rejected("unsupported data URI"))?;
		if encoded.len() > RESOURCE_LIMIT.min(*budget).saturating_add(2) / 3 * 4 {
			return Err(rejected("data URI exceeds byte limit"));
		}
		let bytes = base64::decode(encoded).map_err(rejected)?;
		if bytes.len() > RESOURCE_LIMIT {
			return Err(rejected("resource exceeds 64 MiB"));
		}
		charge(budget, bytes.len())?;
		return Ok(bytes);
	}
	let decoded = urlencoding::decode(uri).map_err(rejected)?;
	// Reject schemes, drive prefixes, UNC paths and platform-dependent separators.
	if decoded.contains([':', '\\', '\0']) || Path::new(decoded.as_ref()).is_absolute() {
		return Err(rejected("only relative asset references are supported"));
	}
	let path = base
		.join(decoded.as_ref())
		.canonicalize()
		.map_err(rejected)?;
	if !path.starts_with(root) {
		return Err(rejected("reference escapes the model directory"));
	}
	read_bounded(&path, budget)
}

pub(super) struct Resources {
	pub(super) source_fingerprint: String,
	document: gltf::Document,
	buffers: Vec<gltf::buffer::Data>,
	images: Vec<ImageResource>,
	budget: usize,
}

enum ImageResource {
	Encoded(Vec<u8>),
	View {
		buffer: usize,
		offset: usize,
		end: usize,
	},
	Decoded(ImageAsset),
}

impl ImageResource {
	fn retained_bytes(&self) -> usize {
		match self {
			Self::Encoded(bytes) => bytes.len(),
			Self::View { .. } => 0,
			Self::Decoded(image) => image.pixels().len(),
		}
	}
}

impl Resources {
	pub(super) fn retained_bytes(&self) -> usize {
		// Every buffer, encoded image and decoded image was charged to the
		// bounded import budget before being retained.
		self.buffers
			.iter()
			.map(|buffer| buffer.0.len())
			.sum::<usize>()
			+ self
				.images
				.iter()
				.map(ImageResource::retained_bytes)
				.sum::<usize>()
	}

	pub(super) fn import_mesh(
		&self,
		mesh_index: usize,
	) -> Result<Vec<ImportedPrimitive>, GltfLoaderError> {
		import_mesh(&self.document, &self.buffers, &self.images, mesh_index)
	}
	/// Authoring publication validates even unused embedded images. Runtime
	/// scene loading retains the cheaper selected-mesh image preparation path.
	pub(super) fn prepare_all_images(&mut self) -> Result<(), GltfLoaderError> {
		let mut budget = self.budget;
		let mut prepared = Vec::new();
		for (index, image) in self.images.iter().enumerate() {
			let encoded = match image {
				ImageResource::Encoded(bytes) => bytes.as_slice(),
				ImageResource::View {
					buffer,
					offset,
					end,
				} => &self.buffers[*buffer].0[*offset..*end],
				ImageResource::Decoded(_) => continue,
			};
			prepared.push((index, decode_image(encoded, &mut budget)?));
		}
		for (index, image) in prepared {
			self.images[index] = ImageResource::Decoded(image);
		}
		self.budget = budget;
		Ok(())
	}

	pub(super) fn prepare_mesh_images(&mut self, mesh_index: usize) -> Result<(), GltfLoaderError> {
		preflight_meshes(&self.document, [mesh_index])?;
		let mesh = self
			.document
			.meshes()
			.nth(mesh_index)
			.ok_or_else(|| rejected(format!("mesh index {mesh_index} does not exist")))?;
		let required: HashSet<_> = mesh
			.primitives()
			.filter_map(|primitive| {
				primitive
					.material()
					.pbr_metallic_roughness()
					.base_color_texture()
					.map(|texture| texture.texture().source().index())
			})
			.collect();
		let mut budget = self.budget;
		let mut prepared = Vec::new();
		// Visit image indices in source order, preserving deterministic failures.
		for (index, image) in self.images.iter().enumerate() {
			if !required.contains(&index) || matches!(image, ImageResource::Decoded(_)) {
				continue;
			}
			let encoded = match image {
				ImageResource::Encoded(bytes) => bytes.as_slice(),
				ImageResource::View {
					buffer,
					offset,
					end,
				} => &self.buffers[*buffer].0[*offset..*end],
				ImageResource::Decoded(_) => unreachable!(),
			};
			prepared.push((index, decode_image(encoded, &mut budget)?));
		}
		// Failed decoding leaves the cached snapshot and its accounting intact.
		for (index, image) in prepared {
			self.images[index] = ImageResource::Decoded(image);
		}
		self.budget = budget;
		Ok(())
	}
}

fn load_resources(path: &Path) -> Result<Resources, GltfLoaderError> {
	let path = path.canonicalize().map_err(rejected)?;
	let root = path
		.parent()
		.ok_or_else(|| rejected("missing model directory"))?;
	load_resources_in_root(&path, root)
}

fn contained_model(path: &Path, root: &Path) -> Result<std::path::PathBuf, GltfLoaderError> {
	let path = path.canonicalize().map_err(rejected)?;
	let root = root.canonicalize().map_err(rejected)?;
	if !path.starts_with(&root) {
		return Err(rejected("model escapes asset root"));
	}
	Ok(path)
}

pub(super) fn load_resources_in_root(
	path: &Path,
	root: &Path,
) -> Result<Resources, GltfLoaderError> {
	let path = contained_model(path, root)?;
	let root = root.canonicalize().map_err(rejected)?;
	let base = path
		.parent()
		.ok_or_else(|| rejected("missing model directory"))?;
	let mut budget = IMPORT_LIMIT;
	let bytes = read_bounded(&path, &mut budget)?;
	let source_fingerprint = super::project_assets::fingerprint(&bytes);
	let gltf::Gltf { document, mut blob } = gltf::Gltf::from_slice(&bytes)?;
	drop(bytes);
	if document.nodes().len() > 10_000
		|| document.meshes().len() > 10_000
		|| document.images().len() > 10_000
		|| document.buffers().len() > 10_000
	{
		return Err(rejected(
			"glTF exceeds 10000 nodes, meshes, images or buffers",
		));
	}
	let mut buffers = Vec::new();
	for buffer in document.buffers() {
		if buffer.length() > RESOURCE_LIMIT {
			return Err(rejected("buffer exceeds 64 MiB"));
		}
		let data = match buffer.source() {
			gltf::buffer::Source::Uri(uri) => read_uri_in_root(base, &root, uri, &mut budget)?,
			gltf::buffer::Source::Bin => blob
				.take()
				.ok_or_else(|| rejected("missing binary buffer"))?,
		};
		if data.len() < buffer.length() {
			return Err(rejected("buffer is shorter than declared"));
		}
		buffers.push(gltf::buffer::Data(data));
	}
	let mut images = Vec::with_capacity(document.images().len());
	for image in document.images() {
		let source = match image.source() {
			gltf::image::Source::Uri { uri, .. } => {
				ImageResource::Encoded(read_uri_in_root(base, &root, uri, &mut budget)?)
			}
			gltf::image::Source::View { view, .. } => {
				let end = view
					.offset()
					.checked_add(view.length())
					.ok_or_else(|| rejected("image range overflow"))?;
				buffers
					.get(view.buffer().index())
					.and_then(|b| b.0.get(view.offset()..end))
					.ok_or_else(|| rejected("image view outside buffer"))?;
				ImageResource::View {
					buffer: view.buffer().index(),
					offset: view.offset(),
					end,
				}
			}
		};
		images.push(source);
	}
	Ok(Resources {
		source_fingerprint,
		document,
		buffers,
		images,
		budget,
	})
}

fn decode_image(encoded: &[u8], budget: &mut usize) -> Result<ImageAsset, GltfLoaderError> {
	let mut reader = image::ImageReader::new(std::io::Cursor::new(encoded))
		.with_guessed_format()
		.map_err(rejected)?;
	let mut limits = image::Limits::default();
	limits.max_image_width = Some(4096);
	limits.max_image_height = Some(4096);
	limits.max_alloc = Some(RESOURCE_LIMIT.min(*budget) as u64);
	reader.limits(limits);
	let decoded = reader.decode().map_err(rejected)?;
	let size = (decoded.width() as usize)
		.checked_mul(decoded.height() as usize)
		.and_then(|n| n.checked_mul(4))
		.ok_or_else(|| rejected("image size overflow"))?;
	charge(budget, size)?;
	let rgba = decoded.into_rgba8();
	Ok(ImageAsset::from_rgba8(
		rgba.width(),
		rgba.height(),
		rgba.into_raw(),
	)?)
}

fn import_first_mesh(
	document: &gltf::Document,
	buffers: &[gltf::buffer::Data],
	images: &[ImageResource],
) -> Result<Vec<ImportedPrimitive>, GltfLoaderError> {
	if document.meshes().next().is_none() {
		return Err(GltfLoaderError::NoMeshes);
	}
	import_mesh(document, buffers, images, 0)
}

fn import_mesh(
	document: &gltf::Document,
	buffers: &[gltf::buffer::Data],
	images: &[ImageResource],
	mesh_index: usize,
) -> Result<Vec<ImportedPrimitive>, GltfLoaderError> {
	preflight_meshes(document, [mesh_index])?;
	let mesh = document
		.meshes()
		.nth(mesh_index)
		.ok_or_else(|| rejected(format!("mesh index {mesh_index} does not exist")))?;
	if mesh.primitives().len() > 4096 {
		return Err(rejected("mesh exceeds 4096 primitives"));
	}
	let mut parts = Vec::new();
	let mut budget = IMPORT_LIMIT;
	let mut shared_images = HashSet::new();
	for primitive in mesh.primitives() {
		// Accessor counts are checked before allocating decoded vertex buffers.
		let vertices = primitive
			.get(&gltf::Semantic::Positions)
			.map_or(0, |a| a.count());
		let indices = primitive.indices().map_or(vertices, |a| a.count());
		let vertex_bytes = vertices
			.checked_mul(std::mem::size_of::<Vertex>() + 48)
			.ok_or_else(|| rejected("geometry size overflow"))?;
		let index_bytes = indices
			.checked_mul(std::mem::size_of::<u32>())
			.ok_or_else(|| rejected("geometry size overflow"))?;
		charge(
			&mut budget,
			vertex_bytes
				.checked_add(index_bytes)
				.ok_or_else(|| rejected("geometry size overflow"))?,
		)?;
		parts.push(import_primitive(
			mesh.index(),
			primitive,
			buffers,
			images,
			&mut shared_images,
			&mut budget,
		)?);
	}
	if parts.is_empty() {
		return Err(GltfLoaderError::NoPrimitives);
	}
	Ok(parts)
}

/// Charge every decoded primitive once per selected mesh. Shared accessors
/// still expand into separate CPU/GPU primitive arrays and must be charged
/// again; repeated scene nodes referencing the same mesh are deduplicated.
pub(super) fn preflight_meshes(
	document: &gltf::Document,
	mesh_indices: impl IntoIterator<Item = usize>,
) -> Result<usize, GltfLoaderError> {
	let mut selected = vec![false; document.meshes().len()];
	for index in mesh_indices {
		*selected
			.get_mut(index)
			.ok_or_else(|| rejected(format!("mesh index {index} does not exist")))? = true;
	}
	let mut budget = IMPORT_LIMIT;
	for mesh in document.meshes().filter(|mesh| selected[mesh.index()]) {
		if mesh.primitives().len() > 4096 {
			return Err(rejected("mesh exceeds 4096 primitives"));
		}
		for primitive in mesh.primitives() {
			let vertices = primitive
				.get(&gltf::Semantic::Positions)
				.ok_or(GltfLoaderError::MissingPositions)?
				.count();
			let indices = primitive
				.indices()
				.map_or(vertices, |accessor| accessor.count());
			if vertices == 0 || indices == 0 {
				return Err(GltfLoaderError::EmptyGeometry);
			}
			if primitive
				.get(&gltf::Semantic::Normals)
				.is_some_and(|accessor| accessor.count() != vertices)
			{
				return Err(rejected("NORMAL count must match POSITION before decoding"));
			}
			let uv_set = primitive
				.material()
				.pbr_metallic_roughness()
				.base_color_texture()
				.map_or(0, |texture| texture.tex_coord());
			if let Some(accessor) = primitive.get(&gltf::Semantic::TexCoords(uv_set))
				&& accessor.count() != vertices
			{
				return Err(GltfLoaderError::TexCoordCountMismatch {
					positions: vertices,
					tex_coords: accessor.count(),
				});
			}
			let bytes = vertices
				.checked_mul(std::mem::size_of::<Vertex>() + 48)
				.and_then(|bytes| {
					indices
						.checked_mul(std::mem::size_of::<u32>())
						.and_then(|indices| bytes.checked_add(indices))
				})
				.ok_or_else(|| rejected("decoded geometry size overflow"))?;
			charge(&mut budget, bytes)?;
		}
	}
	Ok(IMPORT_LIMIT - budget)
}

fn import_primitive(
	mesh_index: usize,
	primitive: gltf::Primitive<'_>,
	buffers: &[gltf::buffer::Data],
	images: &[ImageResource],
	shared_images: &mut HashSet<usize>,
	budget: &mut usize,
) -> Result<ImportedPrimitive, GltfLoaderError> {
	if primitive.mode() != Mode::Triangles {
		return Err(GltfLoaderError::UnsupportedMode(primitive.mode()));
	}
	if primitive.morph_targets().next().is_some() {
		return Err(rejected(
			"morph-target primitives are not supported by static mesh import",
		));
	}

	let primitive_index = primitive.index();

	let material = primitive.material();
	let pbr = material.pbr_metallic_roughness();

	let base_color_factor = pbr.base_color_factor();
	let base_color_texture = pbr.base_color_texture();

	let tex_coord_set = base_color_texture
		.as_ref()
		.map(|info| info.tex_coord())
		.unwrap_or(0);

	engine_logger::debug!(
		"Primitive {}: color factor {:?}, double-sided {}",
		primitive_index,
		base_color_factor,
		material.double_sided()
	);

	let base_color_image_index = base_color_texture
		.as_ref()
		.map(|info| info.texture().source().index());

	let base_color_image = match base_color_texture {
		Some(info) => {
			let image_index = info.texture().source().index();
			let Some(ImageResource::Decoded(image)) = images.get(image_index) else {
				return Err(GltfLoaderError::InvalidImageReference(image_index));
			};
			if shared_images.insert(image_index) {
				// Preserve the per-mesh geometry/image budget even though pixels
				// now share the file cache's allocation across meshes.
				charge(budget, image.pixels().len())?;
			}

			engine_logger::debug!(
				"Primitive {}: image {}, {}x{}, {} RGBA bytes, UV set {}",
				primitive_index,
				image_index,
				image.width(),
				image.height(),
				image.pixels().len(),
				tex_coord_set,
			);

			Some(image.clone())
		}

		None => {
			engine_logger::debug!("Primitive {} uses base-color factor only", primitive_index);

			None
		}
	};

	let reader =
		primitive.reader(|buffer| buffers.get(buffer.index()).map(|data| data.0.as_slice()));

	let positions: Vec<[f32; 3]> = reader
		.read_positions()
		.ok_or(GltfLoaderError::MissingPositions)?
		.collect();

	let indices: Vec<u32> = match reader.read_indices() {
		Some(indices) => indices.into_u32().collect(),
		None => {
			let count = u32::try_from(positions.len())
				.map_err(|_| rejected("mesh exceeds u32 vertex indices"))?;
			(0..count).collect()
		}
	};

	if positions.is_empty() || indices.is_empty() {
		return Err(GltfLoaderError::EmptyGeometry);
	}

	let tex_coords: Vec<[f32; 2]> = match reader.read_tex_coords(tex_coord_set) {
		Some(values) => values.into_f32().collect(),

		None if base_color_image.is_some() => {
			return Err(GltfLoaderError::MissingTexCoords(tex_coord_set));
		}

		None => vec![[0.0, 0.0]; positions.len()],
	};

	if positions.len() != tex_coords.len() {
		return Err(GltfLoaderError::TexCoordCountMismatch {
			positions: positions.len(),
			tex_coords: tex_coords.len(),
		});
	}
	if positions
		.iter()
		.flatten()
		.chain(tex_coords.iter().flatten())
		.any(|value| !value.is_finite())
	{
		return Err(rejected("positions and texture coordinates must be finite"));
	}

	if !indices.len().is_multiple_of(3) {
		return Err(GltfLoaderError::InvalidTriangleIndexCount(indices.len()));
	}

	for &index in &indices {
		let is_in_bounds = usize::try_from(index)
			.map(|index| index < positions.len())
			.unwrap_or(false);

		if !is_in_bounds {
			return Err(GltfLoaderError::IndexOutOfBounds {
				index,
				vertex_count: positions.len(),
			});
		}
	}

	let normals = reader
		.read_normals()
		.map(|values| values.collect::<Vec<_>>());
	if let Some(normals) = &normals
		&& (normals.len() != positions.len()
			|| normals.iter().any(|normal| {
				let normal = glam::Vec3::from_array(*normal);
				!normal.is_finite()
					|| !normal.length_squared().is_finite()
					|| normal.length_squared() < 1.0e-12
			})) {
		return Err(rejected(
			"NORMAL must contain one finite nonzero vector per position",
		));
	}
	let vertices: Vec<Vertex> = positions
		.into_iter()
		.zip(tex_coords)
		.enumerate()
		.map(|(index, (position, uv))| {
			Vertex::new(position, uv).with_normal(normals.as_ref().map_or([0.0; 3], |normals| {
				glam::Vec3::from_array(normals[index])
					.normalize()
					.to_array()
			}))
		})
		.collect();

	engine_logger::debug!(
		"Imported primitive {}: {} vertices, {} indices, {} triangles",
		primitive_index,
		vertices.len(),
		indices.len(),
		indices.len() / 3
	);

	Ok(ImportedPrimitive {
		mesh_index,
		primitive_index,
		base_color_image_index,
		mesh: MeshData::new(vertices, indices),
		material: MaterialData {
			base_color_image,
			base_color_factor,
			double_sided: material.double_sided(),
		},
	})
}
