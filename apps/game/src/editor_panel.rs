use renderer::{
	WindowRenderer,
	vulkan::{ModelPushConstants, RenderObject, Vertex, VulkanMaterial, VulkanMesh},
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
		let mut pixels = vec![0; 128 * 64 * 4];
		for code in 0..128 {
			let glyph = glyph(code as u8 as char);
			for (y, row) in glyph.iter().enumerate() {
				for x in 0..5 {
					if row & (1 << (4 - x)) != 0 {
						let index = (((code / 16) * 8 + y) * 128 + (code % 16) * 8 + x) * 4;
						pixels[index..index + 4].fill(255);
					}
				}
			}
		}
		pixels[..4].fill(255); // Solid rectangle UV.
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
		self.size = [
			extent[0] as f32 / scale as f32,
			extent[1] as f32 / scale as f32,
		];
		self.hits.clear();
		if !self.visible {
			return Ok(());
		}
		let [width, height] = self.size;
		// Compact windows keep the viewport by hiding panels; F1 restores them.
		if width < 800.0 || height < 400.0 {
			return Ok(());
		}
		let mut draw = Draw::new(self.size);
		draw.rect([0.0, 0.0, width, 44.0], BG);
		draw.text(
			14.0,
			14.0,
			if view.dirty {
				"SCENE * UNSAVED"
			} else {
				"SCENE SAVED"
			},
			TEXT,
		);
		self.button(
			&mut draw,
			[260.0, 7.0, 100.0, 30.0],
			if view.playing {
				"STOP [F6]"
			} else {
				"PLAY [F6]"
			},
			PanelAction::TogglePlay,
			false,
		);
		self.button(
			&mut draw,
			[368.0, 7.0, 92.0, 30.0],
			"SAVE F5",
			PanelAction::Save,
			false,
		);
		self.button(
			&mut draw,
			[468.0, 7.0, 104.0, 30.0],
			"RELOAD F9",
			PanelAction::Reload,
			false,
		);
		if !view.playing {
			for (i, (label, mode)) in [
				("1 MOVE", GizmoMode::Translate),
				("2 ROTATE", GizmoMode::Rotate),
				("3 SCALE", GizmoMode::Scale),
			]
			.into_iter()
			.enumerate()
			{
				self.button(
					&mut draw,
					[584.0 + i as f32 * 100.0, 7.0, 94.0, 30.0],
					label,
					PanelAction::Mode(mode),
					self.mode == mode,
				);
			}
		}
		draw.rect([0.0, 48.0, 258.0, height - 78.0], BG);
		draw.text(14.0, 60.0, "HIERARCHY", ACCENT);
		let ids = view.scene.entity_ids();
		let rows = ((height * 0.5 - 98.0) / 26.0).max(1.0) as usize;
		self.entity_scroll = self.entity_scroll.min(ids.len().saturating_sub(rows));
		for (row, id) in ids.skip(self.entity_scroll).take(rows).enumerate() {
			let name = clipped(view.scene.name(id).unwrap_or("Entity"), 19);
			let y = 86.0 + row as f32 * 26.0;
			if view.playing {
				draw.text(14.0, y + 6.0, &name, MUTED);
			} else {
				self.button(
					&mut draw,
					[8.0, y, 242.0, 24.0],
					&name,
					PanelAction::Select(id),
					view.editor.selected_entity() == Some(id),
				);
			}
		}
		let top = height * 0.5;
		draw.text(14.0, top + 8.0, "MODEL ASSETS", ACCENT);
		if !view.playing {
			self.button(
				&mut draw,
				[154.0, top + 2.0, 94.0, 26.0],
				"REFRESH",
				PanelAction::RefreshAssets,
				false,
			);
		}
		let rows = ((height - top - 80.0) / 44.0).max(1.0) as usize;
		self.asset_scroll = self
			.asset_scroll
			.min(view.models.len().saturating_sub(rows));
		for (row, model) in view
			.models
			.iter()
			.skip(self.asset_scroll)
			.take(rows)
			.enumerate()
		{
			let y = top + 38.0 + row as f32 * 44.0;
			draw.text(14.0, y, &clipped(&model.path, 20), TEXT);
			if !view.playing {
				self.button(
					&mut draw,
					[14.0, y + 17.0, 220.0, 23.0],
					"PLACE MODEL",
					PanelAction::Place(model.path.clone()),
					false,
				);
			}
		}
		let right = width - 282.0;
		draw.rect([right, 48.0, 282.0, height - 78.0], BG);
		draw.text(right + 14.0, 60.0, "ENTITY PROPERTIES", ACCENT);
		if let Some(id) = view.editor.selected_entity()
			&& let Some(instance) = view.scene.get(id)
		{
			let transform = instance.transform();
			let name = view.scene.name(id).unwrap_or("");
			self.field(
				&mut draw,
				[right, 88.0],
				"NAME",
				name,
				Property::Name,
				!view.playing,
			);
			draw.text(right + 14.0, 140.0, "MODEL", MUTED);
			draw.text(
				right + 14.0,
				160.0,
				&clipped(view.scene.model_path(id).unwrap_or("None"), 21),
				TEXT,
			);
			draw.text(right + 14.0, 188.0, "POSITION / WORLD", ACCENT);
			for axis in 0..3 {
				self.field(
					&mut draw,
					[right, 210.0 + axis as f32 * 32.0],
					["X", "Y", "Z"][axis],
					&format!("{:.4}", transform.position[axis]),
					Property::Position(axis),
					!view.playing,
				);
			}
			draw.text(right + 14.0, 314.0, "ROTATION / DEGREES", ACCENT);
			for axis in 0..3 {
				self.field(
					&mut draw,
					[right, 336.0 + axis as f32 * 32.0],
					["X", "Y", "Z"][axis],
					&format!("{:.3}", transform.rotation[axis].to_degrees()),
					Property::Rotation(axis),
					!view.playing,
				);
			}
			draw.text(right + 14.0, 440.0, "SCALE / LOCAL", ACCENT);
			for axis in 0..3 {
				self.field(
					&mut draw,
					[right, 462.0 + axis as f32 * 32.0],
					["X", "Y", "Z"][axis],
					&format!("{:.4}", transform.scale[axis]),
					Property::Scale(axis),
					!view.playing,
				);
			}
		} else {
			draw.text(right + 14.0, 96.0, "SELECT AN ENTITY", MUTED);
		}
		if let Some(edit) = &self.edit {
			draw.rect([right + 8.0, height - 102.0, 266.0, 64.0], SELECT);
			draw.text(
				right + 14.0,
				height - 94.0,
				&clipped(&format!("> {}_", edit.text), 21),
				TEXT,
			);
			draw.text(
				right + 14.0,
				height - 70.0,
				"ENTER APPLY / ESC CANCEL",
				ACCENT,
			);
		}
		draw.rect([262.0, height - 82.0, (right - 266.0).max(0.0), 48.0], BG);
		draw.text(
			270.0,
			height - 74.0,
			&format!("FRAME {:.1}MS UPDATE {:.1}MS", view.frame_ms, view.cpu_ms),
			TEXT,
		);
		draw.text(
			270.0,
			height - 54.0,
			&format!(
				"DRAW {} TRI {} CULLED {}/{} [{}]",
				view.stats.draw_calls,
				view.stats.triangles,
				view.stats.culled_entities,
				view.stats.total_entities,
				if view.culling { "F7 ON" } else { "F7 OFF" }
			),
			ACCENT,
		);
		draw.rect([0.0, height - 28.0, width, 28.0], BG);
		draw.text(
			14.0,
			height - 21.0,
			&clipped(&self.status, (width / 12.0) as usize - 2),
			TEXT,
		);
		if let Some(gpu) = self.gpu.as_mut() {
			renderer.write_dynamic_mesh(&mut gpu.mesh, &draw.vertices, &draw.indices)?;
		}
		Ok(())
	}

	fn button(
		&mut self,
		draw: &mut Draw,
		rect: [f32; 4],
		label: &str,
		action: PanelAction,
		selected: bool,
	) {
		draw.rect(rect, if selected { SELECT } else { BUTTON });
		draw.text(rect[0] + 6.0, rect[1] + (rect[3] - 14.0) * 0.5, label, TEXT);
		self.hits.push((rect, action));
	}

	fn field(
		&mut self,
		draw: &mut Draw,
		origin: [f32; 2],
		label: &str,
		value: &str,
		property: Property,
		enabled: bool,
	) {
		let [right, y] = origin;
		if y + 30.0 > self.size[1] - 38.0 {
			return;
		}
		if matches!(property, Property::Name) {
			draw.text(right + 14.0, y, label, MUTED);
			if enabled {
				self.button(
					draw,
					[right + 14.0, y + 20.0, 250.0, 26.0],
					&clipped(value, 19),
					PanelAction::Edit(property),
					false,
				);
			} else {
				draw.text(right + 14.0, y + 24.0, &clipped(value, 19), TEXT);
			}
		} else {
			draw.text(right + 14.0, y + 6.0, label, MUTED);
			if enabled {
				self.button(
					draw,
					[right + 42.0, y, 222.0, 27.0],
					&clipped(value, 17),
					PanelAction::Edit(property),
					false,
				);
			} else {
				draw.text(right + 42.0, y + 6.0, value, TEXT);
			}
		}
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

fn clipped(text: &str, length: usize) -> String {
	text.chars().take(length).collect()
}
const BG: [f32; 3] = [0.055, 0.075, 0.11];
const BUTTON: [f32; 3] = [0.10, 0.15, 0.21];
const SELECT: [f32; 3] = [0.12, 0.30, 0.45];
const TEXT: [f32; 3] = [0.86, 0.91, 0.96];
const MUTED: [f32; 3] = [0.48, 0.62, 0.73];
const ACCENT: [f32; 3] = [0.35, 0.82, 0.92];

struct Draw {
	vertices: Vec<Vertex>,
	indices: Vec<u32>,
	size: [f32; 2],
}
impl Draw {
	fn new(size: [f32; 2]) -> Self {
		Self {
			vertices: Vec::new(),
			indices: Vec::new(),
			size,
		}
	}
	fn quad(&mut self, r: [f32; 4], uv: [f32; 4], color: [f32; 3]) {
		let base = self.vertices.len() as u32;
		for [x, y, u, v] in [
			[r[0], r[1], uv[0], uv[1]],
			[r[0] + r[2], r[1], uv[2], uv[1]],
			[r[0] + r[2], r[1] + r[3], uv[2], uv[3]],
			[r[0], r[1] + r[3], uv[0], uv[3]],
		] {
			self.vertices.push(
				Vertex::new(
					[
						x * 2.0 / self.size[0] - 1.0,
						y * 2.0 / self.size[1] - 1.0,
						0.0,
					],
					[u, v],
				)
				.with_normal(color),
			);
		}
		self.indices
			.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
	}
	fn rect(&mut self, r: [f32; 4], color: [f32; 3]) {
		self.quad(r, [0.5 / 128.0, 0.5 / 64.0, 0.5 / 128.0, 0.5 / 64.0], color);
	}
	fn text(&mut self, x: f32, y: f32, text: &str, color: [f32; 3]) {
		for (i, c) in text.chars().enumerate() {
			let code = if c.is_ascii() {
				c as usize
			} else {
				b'?' as usize
			};
			let [u, v] = [(code % 16 * 8) as f32, (code / 16 * 8) as f32];
			self.quad(
				[x + i as f32 * 12.0, y, 10.0, 14.0],
				[u / 128.0, v / 64.0, (u + 5.0) / 128.0, (v + 7.0) / 64.0],
				color,
			);
		}
	}
}

// A small built-in bitmap alphabet keeps the native editor independent of
// platform fonts and additional UI runtimes. Lowercase shares uppercase glyphs.
fn glyph(c: char) -> [u8; 7] {
	match c.to_ascii_uppercase() {
		'A' => [14, 17, 17, 31, 17, 17, 17],
		'B' => [30, 17, 17, 30, 17, 17, 30],
		'C' => [14, 17, 16, 16, 16, 17, 14],
		'D' => [30, 17, 17, 17, 17, 17, 30],
		'E' => [31, 16, 16, 30, 16, 16, 31],
		'F' => [31, 16, 16, 30, 16, 16, 16],
		'G' => [14, 17, 16, 23, 17, 17, 15],
		'H' => [17, 17, 17, 31, 17, 17, 17],
		'I' => [14, 4, 4, 4, 4, 4, 14],
		'J' => [7, 2, 2, 2, 18, 18, 12],
		'K' => [17, 18, 20, 24, 20, 18, 17],
		'L' => [16, 16, 16, 16, 16, 16, 31],
		'M' => [17, 27, 21, 21, 17, 17, 17],
		'N' => [17, 25, 21, 19, 17, 17, 17],
		'O' => [14, 17, 17, 17, 17, 17, 14],
		'P' => [30, 17, 17, 30, 16, 16, 16],
		'Q' => [14, 17, 17, 17, 21, 18, 13],
		'R' => [30, 17, 17, 30, 20, 18, 17],
		'S' => [15, 16, 16, 14, 1, 1, 30],
		'T' => [31, 4, 4, 4, 4, 4, 4],
		'U' => [17, 17, 17, 17, 17, 17, 14],
		'V' => [17, 17, 17, 17, 17, 10, 4],
		'W' => [17, 17, 17, 21, 21, 21, 10],
		'X' => [17, 17, 10, 4, 10, 17, 17],
		'Y' => [17, 17, 10, 4, 4, 4, 4],
		'Z' => [31, 1, 2, 4, 8, 16, 31],
		'0' => [14, 17, 19, 21, 25, 17, 14],
		'1' => [4, 12, 4, 4, 4, 4, 14],
		'2' => [14, 17, 1, 2, 4, 8, 31],
		'3' => [30, 1, 1, 14, 1, 1, 30],
		'4' => [2, 6, 10, 18, 31, 2, 2],
		'5' => [31, 16, 16, 30, 1, 1, 30],
		'6' => [14, 16, 16, 30, 17, 17, 14],
		'7' => [31, 1, 2, 4, 8, 8, 8],
		'8' => [14, 17, 17, 14, 17, 17, 14],
		'9' => [14, 17, 17, 15, 1, 1, 14],
		'.' => [0, 0, 0, 0, 0, 6, 6],
		',' => [0, 0, 0, 0, 6, 6, 4],
		'-' => [0, 0, 0, 31, 0, 0, 0],
		'+' => [0, 4, 4, 31, 4, 4, 0],
		'*' => [0, 21, 14, 31, 14, 21, 0],
		'/' => [1, 2, 2, 4, 8, 8, 16],
		'\\' => [16, 8, 8, 4, 2, 2, 1],
		'[' => [14, 8, 8, 8, 8, 8, 14],
		']' => [14, 2, 2, 2, 2, 2, 14],
		'(' => [2, 4, 8, 8, 8, 4, 2],
		')' => [8, 4, 2, 2, 2, 4, 8],
		':' => [0, 6, 6, 0, 6, 6, 0],
		'_' => [0, 0, 0, 0, 0, 0, 31],
		'>' => [16, 8, 4, 2, 4, 8, 16],
		'=' => [0, 0, 31, 0, 31, 0, 0],
		' ' => [0; 7],
		_ => [14, 17, 1, 2, 4, 0, 4],
	}
}
