use std::{collections::HashSet, path::Path, sync::Arc};

use renderer::{TextureColorSpace, WindowRenderer, vulkan::Vertex};

use super::{AssetManager, AssetManagerError};
use crate::assets::{ImageAsset, MeshData, gltf_loader::ImportedPrimitive};

enum PendingUpload<'a> {
	Mesh {
		id: String,
		data: &'a MeshData,
		bytes: usize,
	},
	Texture {
		id: String,
		image: &'a ImageAsset,
		color_space: TextureColorSpace,
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

impl AssetManager {
	pub(super) fn preflight_model_upload(
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
			for (id, image, _) in primitive_textures(path, part) {
				if !self.image_textures.contains_key(&id) && images.insert(id) {
					bytes = bytes
						.checked_add(image.map_or(4, |image| image.pixels().len()))
						.ok_or_else(|| {
							AssetManagerError::ResourceBudget("texture byte count overflow".into())
						})?;
					entries += 1;
				}
			}
		}
		self.gpu_budget.admit(bytes, entries)
	}

	pub(super) fn upload_model_resources(
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
			for (id, image, color_space) in primitive_textures(path, part) {
				if !self.image_textures.contains_key(&id) && images.insert(id.clone()) {
					pending.push(PendingUpload::Texture {
						id,
						image: image.unwrap_or(&white),
						color_space,
					});
				}
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
					PendingUpload::Texture {
						id,
						image,
						color_space,
					} => {
						self.load_image_texture_in_color_space(
							renderer,
							id.clone(),
							image,
							*color_space,
						)?;
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
						PendingUpload::Texture {
							image, color_space, ..
						} => Some(renderer::Rgba8UploadWithColorSpace {
							width: image.width(),
							height: image.height(),
							pixels: image.pixels(),
							color_space: *color_space,
						}),
						_ => None,
					})
					.collect();
				let uploaded = renderer
					.upload_batch_with_color_space(&meshes, &textures)
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
							self.gpu_budget.track_mesh(&mesh, bytes);
							self.meshes.insert(id.clone(), mesh);
						}
						PendingUpload::Texture { id, .. } => {
							let texture =
								Arc::new(textures.next().expect("one texture per upload"));
							texture.set_debug_label(id);
							self.gpu_budget.track_texture(&texture, bytes);
							self.image_textures.insert(id.clone(), texture);
						}
					}
				}
			}
			remaining = tail;
		}
		Ok(())
	}
}

pub(super) fn normal_image_key(path: &Path, index: usize) -> String {
	format!("{}#normal-image:{index}", path.display())
}

fn primitive_textures<'a>(
	path: &Path,
	part: &'a ImportedPrimitive,
) -> impl Iterator<Item=(String, Option<&'a ImageAsset>, TextureColorSpace)> {
	let color_id = part.base_color_image_index.map_or_else(
		|| "builtin:white".to_owned(),
		|index| format!("{}#image:{index}", path.display()),
	);
	let normal = part.normal_map.as_ref().map(|normal| {
		(
			normal_image_key(path, normal.image_index),
			Some(&normal.image),
			TextureColorSpace::Linear,
		)
	});
	std::iter::once((
		color_id,
		part.material.base_color_image.as_ref(),
		TextureColorSpace::Srgb,
	))
		.chain(normal)
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

pub(super) fn mesh_bytes(data: &MeshData) -> Result<usize, AssetManagerError> {
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
#[path = "../../../tests/unit/assets/asset_manager/upload.rs"]
mod tests;
