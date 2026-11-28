use std::{collections::HashSet, path::Path};

use super::{
	GltfLoaderError, IMPORT_LIMIT, ImportedPrimitive, charge,
	mesh::{import_first_mesh, import_mesh, import_mesh_checked, preflight_meshes},
	rejected,
};
use crate::assets::{ImageAsset, import_budget::ImportBudget};

const RESOURCE_LIMIT: usize = 64 * 1024 * 1024;
const WARM_IMAGE_DECODER_BYTES: usize = 1024 * 1024;

pub(in crate::assets) struct Resources {
	pub(in crate::assets) source_fingerprint: String,
	document: gltf::Document,
	buffers: Vec<gltf::buffer::Data>,
	images: Vec<ImageResource>,
	budget: usize,
}

pub(super) enum ImageResource {
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
	pub(in crate::assets) fn is_embedded_static(&self) -> bool {
		self.document.animations().next().is_none()
			&& self.document.skins().next().is_none()
			&& self.document.buffers().all(|buffer| match buffer.source() {
			gltf::buffer::Source::Bin => true,
			gltf::buffer::Source::Uri(uri) => uri.starts_with("data:"),
		}) && self.document.images().all(|image| match image.source() {
			gltf::image::Source::View { .. } => true,
			gltf::image::Source::Uri { uri, .. } => uri.starts_with("data:"),
		})
	}

	pub(in crate::assets) fn retained_bytes(&self) -> usize {
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

	pub(in crate::assets) fn import_mesh(
		&self,
		mesh_index: usize,
	) -> Result<Vec<ImportedPrimitive>, GltfLoaderError> {
		import_mesh(&self.document, &self.buffers, &self.images, mesh_index)
	}
	/// Authoring publication validates even unused embedded images. Runtime
	/// scene loading retains the cheaper selected-mesh image preparation path.
	pub(in crate::assets) fn prepare_all_images(&mut self) -> Result<(), GltfLoaderError> {
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

	pub(in crate::assets) fn prepare_mesh_images(
		&mut self,
		mesh_index: usize,
	) -> Result<(), GltfLoaderError> {
		self.prepare_mesh_images_checked(mesh_index, None)
	}

	fn prepare_mesh_images_checked(
		&mut self,
		mesh_index: usize,
		mut warm: Option<&mut ImportBudget<'_>>,
	) -> Result<(), GltfLoaderError> {
		if let Some(warm) = warm.as_deref_mut() {
			warm.reserve_product(self.document.meshes().len(), size_of::<bool>())?;
		}
		let geometry_bytes = preflight_meshes(&self.document, [mesh_index])?;
		let mesh = self
			.document
			.meshes()
			.nth(mesh_index)
			.ok_or_else(|| rejected(format!("mesh index {mesh_index} does not exist")))?;
		if let Some(warm) = warm.as_deref_mut() {
			warm.reserve(geometry_bytes)?;
			// Includes the selected primitive array, image-index set, decode
			// work list, and the second selected-mesh validation before import.
			warm.reserve_product(
				mesh.primitives().len(),
				size_of::<ImportedPrimitive>() + 256,
			)?;
			warm.reserve_product(self.document.meshes().len(), size_of::<bool>())?;
		}
		let required: HashSet<_> = mesh
			.primitives()
			.flat_map(|primitive| {
				let material = primitive.material();
				[
					material
						.pbr_metallic_roughness()
						.base_color_texture()
						.map(|texture| texture.texture().source().index()),
					material
						.normal_texture()
						.map(|texture| texture.texture().source().index()),
				]
					.into_iter()
					.flatten()
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
			let image = if let Some(warm) = warm.as_deref_mut() {
				decode_warm_image(encoded, warm)?
			} else {
				decode_image(encoded, &mut budget)?
			};
			prepared.push((index, image));
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

pub(in crate::assets) fn load_resources_in_root(
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
	let source_fingerprint = super::super::project_assets::fingerprint(&bytes);
	let gltf::Gltf { document, blob } = gltf::Gltf::from_slice(&bytes)?;
	drop(bytes);
	build_resources(
		document,
		blob,
		source_fingerprint,
		budget,
		|uri, budget| read_uri_in_root(base, &root, uri, budget),
		&|| Ok(()),
	)
}

pub(in crate::assets) fn load_warm_mesh(
	bytes: Vec<u8>,
	source_fingerprint: String,
	mesh_index: usize,
	warm: &mut ImportBudget<'_>,
) -> Result<Vec<ImportedPrimitive>, GltfLoaderError> {
	let container = gltf::binary::Glb::from_slice(&bytes)?;
	warm.preflight_json(container.json.as_ref())?;
	warm.reserve(container.bin.as_ref().map_or(0, |bytes| bytes.len()))?;
	let gltf::Gltf { document, blob } = gltf::Gltf::from_slice_without_validation(&bytes)?;
	drop(bytes);
	let raw = document.into_json();
	check_warm_position_indices(&raw, warm)?;
	let document = gltf::Document::from_json(raw)?;
	warm.checkpoint()?;
	if document.animations().next().is_some() || document.skins().next().is_some() {
		return Err(rejected("model warming supports static GLB assets"));
	}
	warm.reserve_product(
		document.buffers().len(),
		size_of::<gltf::buffer::Data>() * 2,
	)?;
	warm.reserve_product(document.images().len(), size_of::<ImageResource>())?;
	let mut resources = build_resources(
		document,
		blob,
		source_fingerprint,
		IMPORT_LIMIT,
		|_, _| Err(rejected("model warming requires embedded GLB resources")),
		&|| warm.checkpoint(),
	)?;
	super::mesh::validate_warm_accessor_ranges(
		&resources.document,
		&resources.buffers,
		mesh_index,
		&|| warm.checkpoint(),
	)?;
	resources.prepare_mesh_images_checked(mesh_index, Some(warm))?;
	import_mesh_checked(
		&resources.document,
		&resources.buffers,
		&resources.images,
		mesh_index,
		&|| warm.checkpoint(),
	)
}

fn check_warm_position_indices(
	raw: &gltf::json::Root,
	warm: &ImportBudget<'_>,
) -> Result<(), GltfLoaderError> {
	use gltf::json::{mesh::Semantic, validation::Checked};
	let position = Checked::Valid(Semantic::Positions);
	for primitive in raw.meshes.iter().flat_map(|mesh| &mesh.primitives) {
		warm.checkpoint()?;
		if let Some(index) = primitive.attributes.get(&position) {
			// The dependency's validation hook indexes POSITION directly.
			let accessor = raw
				.accessors
				.get(index.value())
				.ok_or_else(|| rejected("warm primitive POSITION accessor is missing"))?;
			for bounds in [&accessor.min, &accessor.max].into_iter().flatten() {
				if !bounds.as_array().is_some_and(|values| {
					values.len() == 3 && values.iter().all(|value| value.is_number())
				}) {
					return Err(rejected(
						"warm primitive POSITION bounds need three numbers",
					));
				}
			}
		}
	}
	Ok(())
}

fn build_resources(
	document: gltf::Document,
	mut blob: Option<Vec<u8>>,
	source_fingerprint: String,
	mut budget: usize,
	mut load_uri: impl FnMut(&str, &mut usize) -> Result<Vec<u8>, GltfLoaderError>,
	checkpoint: &impl Fn() -> Result<(), GltfLoaderError>,
) -> Result<Resources, GltfLoaderError> {
	if document.nodes().len() > 10_000
		|| document.meshes().len() > 10_000
		|| document.images().len() > 10_000
		|| document.buffers().len() > 10_000
	{
		return Err(rejected(
			"glTF exceeds 10000 nodes, meshes, images or buffers",
		));
	}
	let mut buffers = Vec::with_capacity(document.buffers().len());
	for buffer in document.buffers() {
		checkpoint()?;
		if buffer.length() > RESOURCE_LIMIT {
			return Err(rejected("buffer exceeds 64 MiB"));
		}
		let data = match buffer.source() {
			gltf::buffer::Source::Uri(uri) => load_uri(uri, &mut budget)?,
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
		checkpoint()?;
		let source = match image.source() {
			gltf::image::Source::Uri { uri, .. } => {
				ImageResource::Encoded(load_uri(uri, &mut budget)?)
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
	image_from_dynamic(decoded)
}

fn decode_warm_image(
	encoded: &[u8],
	warm: &mut ImportBudget<'_>,
) -> Result<ImageAsset, GltfLoaderError> {
	use image::ImageDecoder as _;
	// Opening a decoder may copy the encoded input (JPEG) or allocate for
	// ancillary chunks (PNG) even before reading the first image row.
	warm.reserve_product(encoded.len(), 2)?;
	warm.reserve(WARM_IMAGE_DECODER_BYTES)?;
	let format = reader_format(encoded)?;
	let mut reader = image::ImageReader::with_format(std::io::Cursor::new(encoded), format);
	let mut limits = image::Limits::default();
	limits.max_image_width = Some(4096);
	limits.max_image_height = Some(4096);
	limits.max_alloc = Some(WARM_IMAGE_DECODER_BYTES as u64);
	reader.limits(limits);
	let decoder = reader.into_decoder().map_err(rejected)?;
	let (width, height) = decoder.dimensions();
	let pixels = match format {
		image::ImageFormat::Jpeg => {
			// Charge padded MCU storage, coefficient tables, reconstructed
			// pixels and conversion output together for progressive JPEG.
			(width as usize).next_multiple_of(32) * (height as usize).next_multiple_of(32) * 32
		}
		_ => width as usize * height as usize * 12,
	};
	warm.reserve(pixels)?;
	let decoded = image::DynamicImage::from_decoder(decoder).map_err(rejected)?;
	warm.checkpoint()?;
	image_from_dynamic(decoded)
}

fn reader_format(encoded: &[u8]) -> Result<image::ImageFormat, GltfLoaderError> {
	match image::guess_format(encoded).map_err(rejected)? {
		format @ (image::ImageFormat::Png | image::ImageFormat::Jpeg) => Ok(format),
		_ => Err(rejected(
			"model warming supports embedded PNG and JPEG images",
		)),
	}
}

fn image_from_dynamic(decoded: image::DynamicImage) -> Result<ImageAsset, GltfLoaderError> {
	let rgba = decoded.into_rgba8();
	Ok(ImageAsset::from_rgba8(
		rgba.width(),
		rgba.height(),
		rgba.into_raw(),
	)?)
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

#[cfg(test)]
#[path = "../../../tests/unit/assets/gltf_loader/resources.rs"]
mod tests;
