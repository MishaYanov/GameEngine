//! Bounded bitmap geometry for native screen overlays.
use crate::{
	WindowRenderer,
	vulkan::{ModelPushConstants, RenderObject, Vertex, VulkanMaterial, VulkanMesh},
};
use std::sync::Arc;

pub struct ScreenOverlay {
	mesh: VulkanMesh,
	material: VulkanMaterial,
	prepared: Option<(Vec<String>, [u32; 2], u64)>,
	visible: bool,
}
impl ScreenOverlay {
	pub fn new(renderer: &WindowRenderer) -> Result<Self, String> {
		let texture = renderer
			.upload_rgba8(128, 64, &bitmap_font_pixels())
			.map_err(|error| error.to_string())?;
		Ok(Self {
			mesh: renderer.create_dynamic_mesh(8192, 12288)?,
			material: VulkanMaterial::new(Arc::new(texture), true, [1.0; 4]),
			prepared: None,
			visible: false,
		})
	}
	pub fn prepare(
		&mut self,
		renderer: &WindowRenderer,
		extent: [u32; 2],
		scale: f64,
		rows: &[String],
	) -> Result<(), String> {
		if !scale.is_finite() || scale <= 0.0 {
			return Err("UI scale must be finite and positive".into());
		}
		let size = [
			extent[0] as f32 / scale as f32,
			extent[1] as f32 / scale as f32,
		];
		let width = (size[0] - 32.0).clamp(0.0, 760.0);
		let columns = ((width - 24.0).max(0.0) / 12.0) as usize;
		let available_rows = (((size[1] - 32.0).max(0.0) / 22.0) as usize).min(8);
		let rows = rows
			.iter()
			.take(available_rows)
			.map(|row| row.chars().take(columns.min(128)).collect::<String>())
			.collect::<Vec<_>>();
		if self
			.prepared
			.as_ref()
			.is_some_and(|(last, last_extent, last_scale)| {
				*last_extent == extent && *last_scale == scale.to_bits() && last == &rows
			}) {
			return Ok(());
		}
		self.visible = !rows.is_empty() && columns > 0;
		let mut draw = ScreenCanvas::new(size);
		if self.visible {
			draw.rect(
				[16.0, 16.0, width, rows.len() as f32 * 22.0 + 12.0],
				[0.055, 0.075, 0.11],
			);
			for (index, row) in rows.iter().enumerate() {
				draw.text(28.0, 26.0 + index as f32 * 22.0, row, [0.92, 0.95, 1.0]);
			}
		}
		renderer.write_dynamic_mesh(&mut self.mesh, draw.vertices(), draw.indices())?;
		self.prepared = Some((rows, extent, scale.to_bits()));
		Ok(())
	}
	pub fn object(&self) -> Option<RenderObject<'_>> {
		self.visible.then(|| {
			RenderObject::new(&self.mesh, &self.material, ModelPushConstants::identity())
				.screen_space()
		})
	}
}

pub fn bitmap_font_pixels() -> Vec<u8> {
	let mut pixels = vec![0; 128 * 64 * 4];
	for code in 0..128 {
		for (y, row) in glyph(code as u8 as char).iter().enumerate() {
			for x in 0..5 {
				if row & (1 << (4 - x)) != 0 {
					let index = (((code / 16) * 8 + y) * 128 + (code % 16) * 8 + x) * 4;
					pixels[index..index + 4].fill(255);
				}
			}
		}
	}
	pixels[..4].fill(255);
	pixels
}

pub struct ScreenCanvas {
	vertices: Vec<Vertex>,
	indices: Vec<u32>,
	size: [f32; 2],
}
impl ScreenCanvas {
	pub fn vertices(&self) -> &[Vertex] {
		&self.vertices
	}
	pub fn indices(&self) -> &[u32] {
		&self.indices
	}

	pub fn new(size: [f32; 2]) -> Self {
		Self {
			vertices: Vec::new(),
			indices: Vec::new(),
			size,
		}
	}
	pub fn quad(&mut self, r: [f32; 4], uv: [f32; 4], color: [f32; 3]) {
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
	pub fn rect(&mut self, r: [f32; 4], color: [f32; 3]) {
		self.quad(r, [0.5 / 128.0, 0.5 / 64.0, 0.5 / 128.0, 0.5 / 64.0], color);
	}
	pub fn text(&mut self, x: f32, y: f32, text: &str, color: [f32; 3]) {
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
