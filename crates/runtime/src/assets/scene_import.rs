use glam::{EulerRot, Mat4, Quat, Vec3};

use super::gltf_loader::GltfLoaderError;
use crate::Transform;

/// CPU-only hierarchy description. Node indices retain their glTF identity;
/// entries are ordered so a parent appears before every child.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedScene {
	pub scene_index: Option<usize>,
	pub root_node_indices: Vec<usize>,
	pub nodes: Vec<ImportedNode>,
	pub mesh_count: usize,
	/// Local geometry bounds, aligned with original glTF mesh indices.
	pub mesh_bounds: Vec<Option<crate::Bounds3>>,
	pub meshes: Vec<ImportedMesh>,
	pub materials: Vec<ImportedMaterial>,
	/// Unique referenced encoded files. Embedded resources are already counted
	/// in their owning glTF/GLB file; decoded buffers are not counted twice.
	pub referenced_bytes: usize,
	pub decoded_mesh_bytes: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportedMesh {
	pub mesh_index: usize,
	pub name: String,
	pub primitives: Vec<ImportedPrimitiveMetadata>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportedPrimitiveMetadata {
	pub primitive_index: usize,
	pub vertex_count: usize,
	pub index_count: usize,
	pub material_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportedMaterial {
	pub material_index: Option<usize>,
	pub name: String,
	pub base_color_factor: [f32; 4],
	pub double_sided: bool,
	pub base_color_image_index: Option<usize>,
	pub tex_coord_set: u32,
}

impl ImportedScene {
	/// Retained CPU metadata, including vector/string capacities. Source bytes,
	/// decoded images and document buffers are deliberately absent from output.
	pub fn retained_bytes(&self) -> usize {
		std::mem::size_of::<Self>()
			+ self.root_node_indices.capacity() * std::mem::size_of::<usize>()
			+ self.nodes.capacity() * std::mem::size_of::<ImportedNode>()
			+ self
				.nodes
				.iter()
				.map(|node| node.name.capacity())
				.sum::<usize>()
			+ self.mesh_bounds.capacity() * std::mem::size_of::<Option<crate::Bounds3>>()
			+ self.meshes.capacity() * std::mem::size_of::<ImportedMesh>()
			+ self
				.meshes
				.iter()
				.map(|mesh| {
					mesh.name.capacity()
						+ mesh.primitives.capacity()
							* std::mem::size_of::<ImportedPrimitiveMetadata>()
				})
				.sum::<usize>()
			+ self.materials.capacity() * std::mem::size_of::<ImportedMaterial>()
			+ self
				.materials
				.iter()
				.map(|material| material.name.capacity())
				.sum::<usize>()
	}
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportedNode {
	pub node_index: usize,
	pub parent_node_index: Option<usize>,
	pub name: String,
	pub mesh_index: Option<usize>,
	pub local_transform: Transform,
	/// Column-major accumulated matrix. A legal hierarchy can introduce shear
	/// even though each local node transform is representable as TRS.
	pub world_matrix: [f32; 16],
}

fn invalid(message: impl Into<String>) -> GltfLoaderError {
	GltfLoaderError::Resource(message.into())
}

pub(super) fn describe_scene(
	document: &gltf::Document,
	buffers: &[gltf::buffer::Data],
) -> Result<ImportedScene, GltfLoaderError> {
	describe_scene_checked(document, buffers, &|| Ok(()))
}

pub(super) fn describe_scene_checked(
	document: &gltf::Document,
	buffers: &[gltf::buffer::Data],
	checkpoint: &dyn Fn() -> Result<(), GltfLoaderError>,
) -> Result<ImportedScene, GltfLoaderError> {
	checkpoint()?;
	let node_count = document.nodes().len();
	if node_count > 10_000 || document.meshes().len() > 10_000 {
		return Err(invalid("glTF exceeds 10000 nodes or meshes"));
	}
	let scene = document
		.default_scene()
		.or_else(|| document.scenes().next())
		.ok_or_else(|| invalid("glTF has no scene; use mesh import for mesh-only documents"))?;
	let roots: Vec<_> = scene.nodes().collect();
	let mut result = ImportedScene {
		scene_index: Some(scene.index()),
		root_node_indices: roots.iter().map(|node| node.index()).collect(),
		nodes: Vec::new(),
		mesh_count: document.meshes().len(),
		mesh_bounds: Vec::new(),
		meshes: Vec::new(),
		materials: Vec::new(),
		referenced_bytes: 0,
		decoded_mesh_bytes: 0,
	};
	// Explicit stack avoids recursion on untrusted node graphs. A node may only
	// occur once in the selected scene, rejecting cycles and multiple parents.
	let mut visited = vec![false; node_count];
	let mut pending: Vec<_> = roots
		.into_iter()
		.rev()
		.map(|node| (node, None, Mat4::IDENTITY, 0usize))
		.collect();
	while let Some((node, parent_node_index, parent_matrix, depth)) = pending.pop() {
		checkpoint()?;
		if depth > 128 {
			return Err(invalid("glTF hierarchy exceeds 128 levels"));
		}
		let seen = visited
			.get_mut(node.index())
			.ok_or_else(|| invalid("invalid node index"))?;
		if *seen {
			return Err(invalid(
				"glTF scene contains a cycle or a node with multiple parents",
			));
		}
		*seen = true;
		if node.skin().is_some() || node.weights().is_some() {
			return Err(invalid(
				"skinned and morph-target nodes are not supported by static scene import",
			));
		}
		let local_transform = local_transform(&node)?;
		let world = parent_matrix * local_transform.matrix();
		if !world.is_finite() {
			return Err(invalid(format!(
				"node {} accumulated transform is nonfinite",
				node.index()
			)));
		}
		let name = node
			.name()
			.filter(|name| !name.trim().is_empty())
			.map(str::to_owned)
			.unwrap_or_else(|| format!("Node {}", node.index()));
		if name.len() > 1024 {
			return Err(invalid("glTF node name exceeds 1024 bytes"));
		}
		result.nodes.push(ImportedNode {
			node_index: node.index(),
			parent_node_index,
			name,
			mesh_index: node.mesh().map(|mesh| mesh.index()),
			local_transform,
			world_matrix: world.to_cols_array(),
		});
		let children: Vec<_> = node.children().collect();
		for child in children.into_iter().rev() {
			pending.push((child, Some(node.index()), world, depth + 1));
		}
		if pending.len() > 10_000 {
			return Err(invalid("glTF pending hierarchy exceeds 10000 nodes"));
		}
	}
	let selected: Vec<_> = result
		.nodes
		.iter()
		.filter_map(|node| node.mesh_index)
		.collect();
	result.decoded_mesh_bytes =
		super::gltf_loader::preflight_meshes(document, selected.iter().copied())?;
	result.mesh_bounds = mesh_bounds(document, buffers, &selected, checkpoint)?;
	describe_mesh_metadata(document, &selected, &mut result, checkpoint)?;
	Ok(result)
}

pub(super) fn describe_mesh_checked(
	document: &gltf::Document,
	buffers: &[gltf::buffer::Data],
	mesh_index: usize,
	checkpoint: &dyn Fn() -> Result<(), GltfLoaderError>,
) -> Result<ImportedScene, GltfLoaderError> {
	checkpoint()?;
	let decoded_mesh_bytes = super::gltf_loader::preflight_meshes(document, [mesh_index])?;
	let mut result = ImportedScene {
		scene_index: None,
		root_node_indices: Vec::new(),
		nodes: Vec::new(),
		mesh_count: document.meshes().len(),
		mesh_bounds: mesh_bounds(document, buffers, &[mesh_index], checkpoint)?,
		meshes: Vec::new(),
		materials: Vec::new(),
		referenced_bytes: 0,
		decoded_mesh_bytes,
	};
	describe_mesh_metadata(document, &[mesh_index], &mut result, checkpoint)?;
	Ok(result)
}

fn describe_mesh_metadata(
	document: &gltf::Document,
	selected: &[usize],
	result: &mut ImportedScene,
	checkpoint: &dyn Fn() -> Result<(), GltfLoaderError>,
) -> Result<(), GltfLoaderError> {
	let selected: std::collections::HashSet<_> = selected.iter().copied().collect();
	let mut materials = std::collections::HashSet::new();
	for mesh in document
		.meshes()
		.filter(|mesh| selected.contains(&mesh.index()))
	{
		checkpoint()?;
		let name = mesh.name().unwrap_or("Mesh");
		if name.len() > 1024 {
			return Err(invalid("glTF mesh name exceeds 1024 bytes"));
		}
		let mut primitives = Vec::with_capacity(mesh.primitives().len());
		for primitive in mesh.primitives() {
			checkpoint()?;
			let vertex_count = primitive
				.get(&gltf::Semantic::Positions)
				.ok_or(GltfLoaderError::MissingPositions)?
				.count();
			primitives.push(ImportedPrimitiveMetadata {
				primitive_index: primitive.index(),
				vertex_count,
				index_count: primitive
					.indices()
					.map_or(vertex_count, |indices| indices.count()),
				material_index: primitive.material().index(),
			});
			let material = primitive.material();
			if materials.insert(material.index()) {
				let name = material.name().unwrap_or("Material");
				if name.len() > 1024 {
					return Err(invalid("glTF material name exceeds 1024 bytes"));
				}
				let pbr = material.pbr_metallic_roughness();
				let texture = pbr.base_color_texture();
				result.materials.push(ImportedMaterial {
					material_index: material.index(),
					name: name.to_owned(),
					base_color_factor: pbr.base_color_factor(),
					double_sided: material.double_sided(),
					base_color_image_index: texture
						.as_ref()
						.map(|texture| texture.texture().source().index()),
					tex_coord_set: texture.map_or(0, |texture| texture.tex_coord()),
				});
			}
		}
		result.meshes.push(ImportedMesh {
			mesh_index: mesh.index(),
			name: name.to_owned(),
			primitives,
		});
	}
	result
		.materials
		.sort_by_key(|material| material.material_index);
	Ok(())
}

fn mesh_bounds(
	document: &gltf::Document,
	buffers: &[gltf::buffer::Data],
	selected: &[usize],
	checkpoint: &dyn Fn() -> Result<(), GltfLoaderError>,
) -> Result<Vec<Option<crate::Bounds3>>, GltfLoaderError> {
	let mut result = vec![None; document.meshes().len()];
	let selected: std::collections::HashSet<_> = selected.iter().copied().collect();
	for mesh in document.meshes() {
		checkpoint()?;
		if !selected.contains(&mesh.index()) {
			continue;
		}
		let mut bounds: Option<crate::Bounds3> = None;
		if mesh.primitives().len() > 4096 {
			return Err(invalid("mesh exceeds 4096 primitives"));
		}
		for primitive in mesh.primitives() {
			checkpoint()?;
			if primitive.mode() != gltf::mesh::Mode::Triangles {
				return Err(GltfLoaderError::UnsupportedMode(primitive.mode()));
			}
			if primitive.morph_targets().next().is_some() {
				return Err(invalid(
					"morph-target primitives are not supported by static scene import",
				));
			}
			let positions = primitive
				.reader(|buffer| buffers.get(buffer.index()).map(|data| data.0.as_slice()))
				.read_positions()
				.ok_or(GltfLoaderError::MissingPositions)?;
			for (index, position) in positions.enumerate() {
				if index % 1024 == 0 {
					checkpoint()?;
				}
				let point = crate::Bounds3::new(position, position).map_err(invalid)?;
				bounds = Some(bounds.map_or(point, |current| current.union(point)));
			}
		}
		result[mesh.index()] = bounds;
	}
	Ok(result)
}

fn local_transform(node: &gltf::Node<'_>) -> Result<Transform, GltfLoaderError> {
	let (translation, rotation, scale) = match node.transform() {
		gltf::scene::Transform::Decomposed {
			translation,
			rotation,
			scale,
		} => {
			let quaternion = Quat::from_array(rotation);
			if !quaternion.is_finite() || (quaternion.length_squared() - 1.0).abs() > 1.0e-3 {
				return Err(invalid(format!(
					"node {} rotation must be a unit quaternion",
					node.index()
				)));
			}
			(
				Vec3::from_array(translation),
				quaternion.normalize(),
				Vec3::from_array(scale),
			)
		}
		gltf::scene::Transform::Matrix { matrix } => {
			let matrix = Mat4::from_cols_array_2d(&matrix);
			// glam decomposition requires a nonzero determinant. Reject singular
			// matrix representations instead of guessing their rotation.
			if !matrix.is_finite() || matrix.determinant().abs() < 1.0e-12 {
				return Err(invalid(format!(
					"node {} matrix is singular or nonfinite",
					node.index()
				)));
			}
			let (scale, rotation, translation) = matrix.to_scale_rotation_translation();
			let reconstructed = Mat4::from_scale_rotation_translation(scale, rotation, translation);
			if !rotation.is_finite() || !matrix_approximately_equal(matrix, reconstructed) {
				return Err(invalid(format!(
					"node {} matrix has shear or is not representable as TRS",
					node.index()
				)));
			}
			(translation, rotation.normalize(), scale)
		}
	};
	let (x, y, z) = rotation.to_euler(EulerRot::XYZ);
	let transform = Transform {
		position: translation.to_array(),
		rotation: [x, y, z],
		scale: scale.to_array(),
	};
	transform.validate().map_err(invalid)?;
	let source = Mat4::from_cols_array_2d(&node.transform().matrix());
	if !matrix_approximately_equal(source, transform.matrix()) {
		return Err(invalid(format!(
			"node {} transform cannot be represented by engine Euler TRS",
			node.index()
		)));
	}
	Ok(transform)
}

fn matrix_approximately_equal(a: Mat4, b: Mat4) -> bool {
	a.to_cols_array()
		.into_iter()
		.zip(b.to_cols_array())
		.all(|(a, b)| {
			a.is_finite()
				&& b.is_finite()
				&& (a - b).abs() <= 1.0e-4 * a.abs().max(b.abs()).max(1.0)
		})
}
