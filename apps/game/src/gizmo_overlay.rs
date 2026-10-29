use glam::Vec3;
use renderer::{
	WindowRenderer,
	vulkan::{ModelPushConstants, RenderObject, Vertex, VulkanMaterial, VulkanMesh},
};
use runtime::{
	Camera,
	assets::{AssetManager, ImageAsset},
	gizmo::{self, GizmoMode},
};

pub(crate) struct GizmoOverlay {
	mesh: VulkanMesh,
	material: VulkanMaterial,
}

impl GizmoOverlay {
	pub fn new(renderer: &WindowRenderer, assets: &mut AssetManager) -> Result<Self, String> {
		let image = ImageAsset::from_rgba8(1, 1, vec![255; 4]).map_err(|e| e.to_string())?;
		let texture = assets
			.load_image_texture(renderer, "builtin:white", &image)
			.map_err(|e| e.to_string())?;
		Ok(Self {
			mesh: renderer.create_dynamic_mesh(4096, 6144)?,
			material: VulkanMaterial::new(texture, true, [1.0; 4]),
		})
	}

	pub fn prepare(
		&mut self,
		renderer: &WindowRenderer,
		camera: &Camera,
		extent: [u32; 2],
		transform: gizmo::GizmoPose,
		mode: GizmoMode,
		active: Option<usize>,
	) -> Result<(), String> {
		let position = transform
			.parent
			.transform_point3(Vec3::from_array(transform.local.position));
		let size = gizmo::length(camera, extent, position.to_array());
		let thickness = size * 0.012;
		let forward = Vec3::from_array(camera.forward());
		let scale_cap = (mode == GizmoMode::Scale).then(|| {
			let right = Vec3::from_array(camera.right()) * thickness * 3.0;
			let up = right.cross(forward).normalize_or_zero() * thickness * 3.0;
			(right, up)
		});
		let mut vertices = Vec::new();
		let mut indices = Vec::new();
		for segment in gizmo::segments_parented(camera, extent, transform, mode) {
			let direction = (segment.b - segment.a).normalize_or_zero();
			let side = direction.cross(forward).try_normalize().unwrap_or(Vec3::X) * thickness;
			let color = if active == Some(segment.axis) {
				[1.0, 0.85, 0.1]
			} else {
				[[0.95, 0.2, 0.2], [0.2, 0.9, 0.35], [0.25, 0.5, 1.0]][segment.axis]
			};
			let base = vertices.len() as u32;
			for point in [
				segment.a - side,
				segment.a + side,
				segment.b + side,
				segment.b - side,
			] {
				vertices.push(Vertex::new(point.to_array(), [0.0; 2]).with_normal(color));
			}
			indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
			if let Some((right, up)) = scale_cap {
				// A camera-facing square at each local-axis endpoint.
				let base = vertices.len() as u32;
				for point in [
					segment.b - right - up,
					segment.b + right - up,
					segment.b + right + up,
					segment.b - right + up,
				] {
					vertices.push(Vertex::new(point.to_array(), [0.0; 2]).with_normal(color));
				}
				indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
			}
		}
		renderer.write_dynamic_mesh(&mut self.mesh, &vertices, &indices)
	}

	pub fn object(&self) -> RenderObject<'_> {
		let mut transform = ModelPushConstants::identity();
		transform.flags[0] = 1 | 4 | 8;
		RenderObject::new(&self.mesh, &self.material, transform)
	}
}
