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
