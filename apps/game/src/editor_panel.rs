mod layout;

use renderer::ui::{ScreenCanvas as Draw, bitmap_font_pixels};
use renderer::{
	WindowRenderer,
	vulkan::{ModelPushConstants, RenderObject, VulkanMaterial, VulkanMesh},
};
use runtime::{
	SceneEditor,
	assets::{AssetManager, ImageAsset, catalog::ModelAsset},
	gizmo::GizmoMode,
	scene::{EntityId, Scene},
	visibility::SceneRenderStats,
};

#[derive(Clone, Copy)]
pub(crate) enum Property {
	Name,
	Position(usize),
	Rotation(usize),
	Scale(usize),
}

#[derive(Clone)]
pub(crate) enum PanelAction {
	Select(EntityId),
	Edit(Property),
	Mode(GizmoMode),
	Place(String),
	RefreshAssets,
	TogglePlay,
	Save,
	Reload,
}

pub(crate) struct TextEdit {
	pub entity: EntityId,
	pub property: Property,
	pub text: String,
	pub replace: bool,
}

struct UiGpu {
	mesh: VulkanMesh,
	material: VulkanMaterial,
}

pub(crate) struct EditorPanel {
	gpu: Option<UiGpu>,
	pub visible: bool,
	pub mode: GizmoMode,
	pub edit: Option<TextEdit>,
	pub status: String,
	entity_scroll: usize,
	asset_scroll: usize,
	hits: Vec<([f32; 4], PanelAction)>,
	size: [f32; 2],
}

impl Default for EditorPanel {
	fn default() -> Self {
		Self {
			gpu: None,
			visible: true,
			mode: GizmoMode::Translate,
			edit: None,
			status: "Click a field to edit. Enter applies; Escape cancels.".into(),
			entity_scroll: 0,
			asset_scroll: 0,
			hits: Vec::new(),
			size: [0.0; 2],
		}
	}
}

pub(crate) struct PanelView<'a> {
	pub scene: &'a Scene,
	pub editor: &'a SceneEditor,
	pub models: &'a [ModelAsset],
	pub playing: bool,
	pub dirty: bool,
	pub stats: SceneRenderStats,
	pub frame_ms: f32,
	pub cpu_ms: f32,
	pub culling: bool,
}

impl EditorPanel {
	pub fn initialize(
		&mut self,
		renderer: &WindowRenderer,
		assets: &mut AssetManager,
	) -> Result<(), String> {
		let pixels = bitmap_font_pixels();
		let image = ImageAsset::from_rgba8(128, 64, pixels).map_err(|e| e.to_string())?;
		let texture = assets
			.load_image_texture(renderer, "builtin:editor-font", &image)
			.map_err(|e| e.to_string())?;
		self.gpu = Some(UiGpu {
			mesh: renderer.create_dynamic_mesh(16_384, 24_576)?,
			material: VulkanMaterial::new(texture, true, [1.0; 4]),
		});
		Ok(())
	}

	pub fn contains(&self, pixel: [f64; 2], scale: f64) -> bool {
		if !self.visible || self.size[0] < 800.0 || self.size[1] < 400.0 {
			return false;
		}
		let [x, y] = [
			pixel[0] as f32 / scale as f32,
			pixel[1] as f32 / scale as f32,
		];
		y < 44.0 || y > self.size[1] - 30.0 || x < 258.0 || x > self.size[0] - 282.0
	}

	pub fn hit(&self, pixel: [f64; 2], scale: f64) -> Option<PanelAction> {
		let [x, y] = [
			pixel[0] as f32 / scale as f32,
			pixel[1] as f32 / scale as f32,
		];
		self.hits
			.iter()
			.rev()
			.find(|(r, _)| x >= r[0] && y >= r[1] && x < r[0] + r[2] && y < r[1] + r[3])
			.map(|(_, a)| a.clone())
	}

	pub fn scroll(&mut self, pixel: [f64; 2], scale: f64, lines: f32) {
		let [x, y] = [
			pixel[0] as f32 / scale as f32,
			pixel[1] as f32 / scale as f32,
		];
		if x >= 258.0 {
			return;
		}
		let offset = if y < self.size[1] * 0.5 {
			&mut self.entity_scroll
		} else {
			&mut self.asset_scroll
		};
		*offset = (*offset as isize - lines.round() as isize).max(0) as usize;
	}

	pub fn begin_edit(&mut self, scene: &Scene, editor: &SceneEditor, property: Property) {
		let Some(entity) = editor.selected_entity() else {
			return;
		};
		let Some(instance) = scene.get(entity) else {
			return;
		};
		let transform = instance.transform();
		let text = match property {
			Property::Name => scene.name(entity).unwrap_or("").into(),
			Property::Position(axis) => transform.position[axis].to_string(),
			Property::Rotation(axis) => transform.rotation[axis].to_degrees().to_string(),
			Property::Scale(axis) => transform.scale[axis].to_string(),
		};
		self.edit = Some(TextEdit {
			entity,
			property,
			text,
			replace: true,
		});
	}

	pub fn prepare(
		&mut self,
		renderer: &WindowRenderer,
		extent: [u32; 2],
		scale: f64,
		view: PanelView<'_>,
	) -> Result<(), String> {
		let Some(draw) = self.layout(extent, scale, view) else {
			return Ok(());
		};
		if let Some(gpu) = self.gpu.as_mut() {
			renderer.write_dynamic_mesh(&mut gpu.mesh, draw.vertices(), draw.indices())?;
		}
		Ok(())
	}

	pub fn object(&self) -> Option<RenderObject<'_>> {
		if !self.visible || self.size[0] < 800.0 || self.size[1] < 400.0 {
			return None;
		}
		let gpu = self.gpu.as_ref()?;
		Some(
			RenderObject::new(&gpu.mesh, &gpu.material, ModelPushConstants::identity())
				.screen_space(),
		)
	}
}
