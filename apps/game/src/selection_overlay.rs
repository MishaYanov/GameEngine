use renderer::{
	WindowRenderer,
	vulkan::{ModelPushConstants, RenderObject, Vertex, VulkanMaterial, VulkanMesh},
};
use runtime::{
	SceneEditor,
	assets::{AssetManager, ImageAsset, MeshData},
	scene::Scene,
};
use std::sync::Arc;

pub(crate) struct SelectionOverlay {
	mesh: Arc<VulkanMesh>,
	material: VulkanMaterial,
}

impl SelectionOverlay {
	pub(crate) fn new(
		renderer: &WindowRenderer,
		assets: &mut AssetManager,
	) -> Result<Self, String> {
		let (vertices, indices) = wire_box();
		let mesh = assets
			.load_mesh(
				renderer,
				"builtin:selection-box",
				&MeshData::new(vertices, indices),
			)
			.map_err(|error| error.to_string())?;
		let white =
			ImageAsset::from_rgba8(1, 1, vec![255; 4]).map_err(|error| error.to_string())?;
		let texture = assets
			.load_image_texture(renderer, "builtin:white", &white)
			.map_err(|error| error.to_string())?;
		Ok(Self {
			mesh,
			material: VulkanMaterial::new(texture, true, [1.0, 0.8, 0.05, 1.0]),
		})
	}

	pub(crate) fn object<'a>(
		&'a self,
		scene: &Scene,
		editor: &SceneEditor,
	) -> Option<RenderObject<'a>> {
		let selected = editor.selected_entity()?;
		let instance = scene.get(selected)?;
		let bounds = instance.model().bounds()?;
		let min = glam::Vec3::from_array(bounds.min());
		let max = glam::Vec3::from_array(bounds.max());
		// Pad slightly so the bars remain visible beside the model surfaces.
		let size = (max - min).max(glam::Vec3::splat(0.001));
		let padding = size * 0.01;
		let local = glam::Mat4::from_scale_rotation_translation(
			size + padding * 2.0,
			glam::Quat::IDENTITY,
			min - padding,
		);
		let transform = ModelPushConstants {
			model: (scene.world_matrix(selected).ok()? * local).to_cols_array(),
			base_color_factor: [1.0; 4],
			flags: [1, 0, 0, 0],
		};
		Some(RenderObject::new(&self.mesh, &self.material, transform))
	}
}

fn wire_box() -> (Vec<Vertex>, Vec<u32>) {
	let mut vertices = Vec::new();
	let mut indices = Vec::new();
	let thickness = 0.006;
	for axis in 0..3 {
		for side_a in [0.0, 1.0] {
			for side_b in [0.0, 1.0] {
				let mut min = [0.0; 3];
				let mut max = [1.0; 3];
				let a = (axis + 1) % 3;
				let b = (axis + 2) % 3;
				min[a] = side_a - thickness;
				max[a] = side_a + thickness;
				min[b] = side_b - thickness;
				max[b] = side_b + thickness;
				let base = vertices.len() as u32;
				for corner in 0..8 {
					vertices.push(Vertex::new(
						[
							if corner & 1 == 0 { min[0] } else { max[0] },
							if corner & 2 == 0 { min[1] } else { max[1] },
							if corner & 4 == 0 { min[2] } else { max[2] },
						],
						[0.0; 2],
					));
				}
				for index in [
					0, 1, 3, 0, 3, 2, 4, 6, 7, 4, 7, 5, 0, 4, 5, 0, 5, 1, 2, 3, 7, 2, 7, 6, 0, 2,
					6, 0, 6, 4, 1, 5, 7, 1, 7, 3,
				] {
					indices.push(base + index);
				}
			}
		}
	}
	(vertices, indices)
}
