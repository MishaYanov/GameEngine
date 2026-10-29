use std::{mem::size_of_val, sync::Arc};

use ash::Instance;

use super::{Vertex, VulkanDevice, VulkanMaterialLayout, VulkanMesh, VulkanTexture};

/// Bounds staging payload retained by one synchronous batch, excluding GPU alignment.
pub const MAX_UPLOAD_BATCH_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_UPLOAD_BATCH_RESOURCES: usize = 256;

#[derive(Clone, Copy)]
pub struct MeshUpload<'a> {
	pub vertices: &'a [Vertex],
	pub indices: &'a [u32],
}

#[derive(Clone, Copy)]
pub struct Rgba8Upload<'a> {
	pub width: u32,
	pub height: u32,
	pub pixels: &'a [u8],
}

pub struct UploadedBatch {
	pub meshes: Vec<VulkanMesh>,
	pub textures: Vec<VulkanTexture>,
}

fn validate(meshes: &[MeshUpload<'_>], textures: &[Rgba8Upload<'_>]) -> Result<(), String> {
	if meshes.len().saturating_add(textures.len()) > MAX_UPLOAD_BATCH_RESOURCES {
		return Err(format!(
			"Upload batch exceeds {MAX_UPLOAD_BATCH_RESOURCES} resources"
		));
	}
	let mut bytes = 0usize;
	for mesh in meshes {
		if mesh.vertices.is_empty() || mesh.indices.is_empty() {
			return Err("Cannot upload an empty mesh".into());
		}
		u32::try_from(mesh.indices.len()).map_err(|_| "Mesh contains too many indices")?;
		bytes = bytes
			.saturating_add(size_of_val(mesh.vertices))
			.saturating_add(size_of_val(mesh.indices));
	}
	for texture in textures {
		bytes = bytes.saturating_add(
			VulkanTexture::validate_upload(texture.width, texture.height, texture.pixels)
				.map_err(|error| error.to_string())?,
		);
	}
	if bytes > MAX_UPLOAD_BATCH_BYTES {
		return Err(format!(
			"Upload batch exceeds {MAX_UPLOAD_BATCH_BYTES} payload bytes"
		));
	}
	Ok(())
}

pub(crate) fn upload(
	instance: &Instance,
	device: &VulkanDevice,
	layout: &Arc<VulkanMaterialLayout>,
	meshes: &[MeshUpload<'_>],
	textures: &[Rgba8Upload<'_>],
) -> Result<UploadedBatch, String> {
	validate(meshes, textures)?;
	if meshes.is_empty() && textures.is_empty() {
		return Ok(UploadedBatch {
			meshes: Vec::new(),
			textures: Vec::new(),
		});
	}
	let meshes = meshes
		.iter()
		.map(|mesh| {
			VulkanMesh::prepare_upload(instance, device, mesh.vertices, mesh.indices)
				.map_err(|error| error.to_string())
		})
		.collect::<Result<Vec<_>, _>>()?;
	let textures = textures
		.iter()
		.map(|texture| {
			VulkanTexture::prepare_upload(
				instance,
				device,
				Arc::clone(layout),
				texture.width,
				texture.height,
				texture.pixels,
			)
			.map(|(image, staging)| (image, staging, texture.width, texture.height))
			.map_err(|error| error.to_string())
		})
		.collect::<Result<Vec<_>, _>>()?;
	let (meshes, textures) = super::upload::submit(
		device,
		(meshes, textures),
		|(meshes, textures), raw, command| {
			for (mesh, staging) in meshes {
				super::upload::copy_buffer(raw, command, &staging[0], mesh.vertex_buffer());
				super::upload::copy_buffer(raw, command, &staging[1], mesh.index_buffer());
			}
			for (image, staging, width, height) in textures {
				image.record_upload(raw, command, staging, *width, *height);
			}
		},
	)
	.map_err(|error| format!("Vulkan batch upload failed: {error:?}"))?;
	Ok(UploadedBatch {
		meshes: meshes.into_iter().map(|(mesh, _)| mesh).collect(),
		textures: textures
			.into_iter()
			.map(|(texture, _, _, _)| texture)
			.collect(),
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn batch_validation_bounds_combined_resources_and_payload_before_upload() {
		let vertices = [Vertex::new([0.0; 3], [0.0; 2])];
		let indices = [0];
		let mesh = MeshUpload {
			vertices: &vertices,
			indices: &indices,
		};
		let texture = Rgba8Upload {
			width: 1,
			height: 1,
			pixels: &[0; 4],
		};
		assert!(validate(&[], &[]).is_ok());
		assert!(validate(&[mesh; MAX_UPLOAD_BATCH_RESOURCES - 1], &[texture]).is_ok());
		assert!(
			validate(&[mesh; MAX_UPLOAD_BATCH_RESOURCES], &[texture])
				.unwrap_err()
				.contains("resources")
		);
		let pixels = vec![0; 4 * 1024 * 1024];
		let large = Rgba8Upload {
			width: 1024,
			height: 1024,
			pixels: &pixels,
		};
		assert!(validate(&[], &[large; 16]).is_ok());
		assert!(
			validate(&[mesh], &[large; 16])
				.unwrap_err()
				.contains("payload bytes")
		);
	}

	#[test]
	fn batch_validation_rejects_empty_meshes_and_invalid_texture_sizes() {
		assert!(
			validate(
				&[MeshUpload {
					vertices: &[],
					indices: &[0]
				}],
				&[]
			)
			.is_err()
		);
		for (width, height, pixels) in [
			(0, 1, &[][..]),
			(1, 0, &[][..]),
			(u32::MAX, u32::MAX, &[][..]),
			(1, 1, &[0; 3][..]),
		] {
			assert!(
				validate(
					&[],
					&[Rgba8Upload {
						width,
						height,
						pixels
					}]
				)
				.is_err()
			);
		}
	}
}
