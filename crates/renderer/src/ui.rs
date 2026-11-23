//! Bounded bitmap geometry for native screen overlays.
use crate::{
	WindowRenderer,
	vulkan::{ModelPushConstants, RenderObject, Vertex, VulkanMaterial, VulkanMesh},
};
use std::sync::Arc;

const COLUMN_WIDTH: f32 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenOverlayStyle {
	pub text_scale: f32,
	pub high_contrast: bool,
}

impl ScreenOverlayStyle {
	fn colors(self) -> ([f32; 3], [f32; 3]) {
		if self.high_contrast {
			([0.0; 3], [1.0; 3])
		} else {
			([0.055, 0.075, 0.11], [0.92, 0.95, 1.0])
		}
	}
}

impl Default for ScreenOverlayStyle {
	fn default() -> Self {
		Self {
			text_scale: 1.0,
			high_contrast: false,
		}
	}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScreenOverlayCapacity {
	pub rows: usize,
	pub columns: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenOverlayLayout {
	Standard,
	Compact { bottom: bool, aim: bool },
}

struct OverlayLayout {
	size: [f32; 2],
	width: f32,
	row_height: f32,
	capacity: ScreenOverlayCapacity,
}

impl OverlayLayout {
	const MARGIN: f32 = 16.0;
	const SIDE_PADDING: f32 = 12.0;
	const TOP_PADDING: f32 = 10.0;
	const VERTICAL_PADDING: f32 = 12.0;
	const BOTTOM_MARGIN: f32 = 4.0;

	fn new(extent: [u32; 2], dpi: f64, style: ScreenOverlayStyle) -> Option<Self> {
		if !dpi.is_finite() || dpi <= 0.0 || !(1.0..=1.5).contains(&style.text_scale) {
			return None;
		}

		let size = extent.map(|dimension| (f64::from(dimension) / dpi) as f32);
		if size.iter().any(|dimension| !dimension.is_finite()) {
			return None;
		}
		let width = (size[0] - 2.0 * Self::MARGIN).clamp(0.0, 760.0);
		let row_height = 22.0 * style.text_scale;
		let columns = ((width - 2.0 * Self::SIDE_PADDING).max(0.0)
			/ (COLUMN_WIDTH * style.text_scale)) as usize;
		let usable_height =
			(size[1] - Self::MARGIN - Self::VERTICAL_PADDING - Self::BOTTOM_MARGIN).max(0.0);
		let rows = if columns == 0 {
			0
		} else {
			((usable_height / row_height) as usize).min(8)
		};

		Some(Self {
			size,
			width,
			row_height,
			capacity: ScreenOverlayCapacity {
				rows,
				columns: columns.min(128),
			},
		})
	}

	fn draw(
		&self,
		rows: &[String],
		style: ScreenOverlayStyle,
		layout: ScreenOverlayLayout,
	) -> ScreenCanvas {
		let mut canvas = ScreenCanvas::new(self.size);
		if !rows.is_empty() {
			let (mut background, mut text) = style.colors();
			let height = rows.len() as f32 * self.row_height + Self::VERTICAL_PADDING;
			let (width, top) = match layout {
				ScreenOverlayLayout::Standard => (self.width, Self::MARGIN),
				ScreenOverlayLayout::Compact { bottom, .. } => {
					let longest = rows
						.iter()
						.map(|row| row.chars().count())
						.max()
						.unwrap_or(0);
					let width = (longest as f32 * COLUMN_WIDTH * style.text_scale
						+ 2.0 * Self::SIDE_PADDING)
						.min(self.width);
					if !style.high_contrast {
						background = [0.025, 0.032, 0.043];
						text = [0.99, 0.94, 0.84];
					}
					let top = if bottom {
						self.size[1] - Self::MARGIN - height
					} else {
						Self::MARGIN
					};
					(width, top)
				}
			};
			canvas.rect([Self::MARGIN, top, width, height], background);
			if matches!(layout, ScreenOverlayLayout::Compact { .. }) {
				let accent = if style.high_contrast {
					[1.0; 3]
				} else {
					[0.80, 0.50, 0.24]
				};
				canvas.rect([Self::MARGIN, top, 2.0, height], accent);
			}
			for (index, row) in rows.iter().enumerate() {
				canvas.text_scaled(
					Self::MARGIN + Self::SIDE_PADDING,
					top + Self::TOP_PADDING + index as f32 * self.row_height,
					row,
					text,
					style.text_scale,
				);
			}
		}
		canvas
	}
}

#[derive(Clone, Copy, PartialEq)]
struct OverlayKey {
	extent: [u32; 2],
	dpi_bits: u64,
	style: ScreenOverlayStyle,
	layout: ScreenOverlayLayout,
}

struct PreparedOverlay {
	key: OverlayKey,
	rows: Vec<String>,
}

impl PreparedOverlay {
	fn matches(&self, key: OverlayKey, rows: &[String], columns: usize) -> bool {
		self.key == key
			&& self.rows.len() == rows.len()
			&& self
				.rows
				.iter()
				.zip(rows)
				.all(|(before, now)| before == now || before.chars().eq(now.chars().take(columns)))
	}
}

pub struct ScreenOverlay {
	mesh: VulkanMesh,
	material: VulkanMaterial,
	prepared: Option<PreparedOverlay>,
}

impl ScreenOverlay {
	/// Number of text rows available with the default style.
	pub fn row_capacity(extent: [u32; 2], dpi: f64) -> usize {
		Self::capacity_with_style(extent, dpi, ScreenOverlayStyle::default()).rows
	}

	/// Text capacity uses the same logical geometry as overlay preparation.
	pub fn capacity_with_style(
		extent: [u32; 2],
		dpi: f64,
		style: ScreenOverlayStyle,
	) -> ScreenOverlayCapacity {
		OverlayLayout::new(extent, dpi, style)
			.map_or_else(ScreenOverlayCapacity::default, |layout| layout.capacity)
	}

	pub fn new(renderer: &WindowRenderer) -> Result<Self, String> {
		let texture = renderer
			.upload_rgba8(128, 64, &bitmap_font_pixels())
			.map_err(|error| error.to_string())?;
		Ok(Self {
			mesh: renderer.create_dynamic_mesh(8192, 12288)?,
			material: VulkanMaterial::new(Arc::new(texture), true, [1.0; 4]),
			prepared: None,
		})
	}

	pub fn prepare(
		&mut self,
		renderer: &WindowRenderer,
		extent: [u32; 2],
		dpi: f64,
		rows: &[String],
	) -> Result<(), String> {
		self.prepare_with_style(renderer, extent, dpi, rows, ScreenOverlayStyle::default())
	}

	pub fn prepare_with_style(
		&mut self,
		renderer: &WindowRenderer,
		extent: [u32; 2],
		dpi: f64,
		rows: &[String],
		style: ScreenOverlayStyle,
	) -> Result<(), String> {
		self.prepare_with_layout(
			renderer,
			extent,
			dpi,
			rows,
			style,
			ScreenOverlayLayout::Standard,
		)
	}

	/// Draw a compact gameplay panel and optional center aiming mark.
	pub fn prepare_with_layout(
		&mut self,
		renderer: &WindowRenderer,
		extent: [u32; 2],
		dpi: f64,
		rows: &[String],
		style: ScreenOverlayStyle,
		screen_layout: ScreenOverlayLayout,
	) -> Result<(), String> {
		let layout = OverlayLayout::new(extent, dpi, style)
			.ok_or("Overlay requires finite DPI geometry and a text scale from 1 to 1.5")?;
		let key = OverlayKey {
			extent,
			dpi_bits: dpi.to_bits(),
			style,
			layout: screen_layout,
		};
		let rows = &rows[..layout.capacity.rows.min(rows.len())];
		let columns = layout.capacity.columns;
		if self
			.prepared
			.as_ref()
			.is_some_and(|last| last.matches(key, rows, columns))
		{
			return Ok(());
		}

		let rows = rows
			.iter()
			.map(|row| row.chars().take(columns).collect::<String>())
			.collect::<Vec<_>>();
		let mut draw = layout.draw(&rows, style, screen_layout);
		if matches!(
			screen_layout,
			ScreenOverlayLayout::Compact { aim: true, .. }
		) {
			let [x, y] = layout.size.map(|value| value * 0.5);
			draw.rect([x - 7.0, y - 1.5, 14.0, 3.0], [0.06, 0.045, 0.032]);
			draw.rect([x - 1.5, y - 7.0, 3.0, 14.0], [0.06, 0.045, 0.032]);
			draw.rect([x - 6.0, y - 0.6, 12.0, 1.2], [1.0, 0.86, 0.62]);
			draw.rect([x - 0.6, y - 6.0, 1.2, 12.0], [1.0, 0.86, 0.62]);
		}
		renderer.write_dynamic_mesh(&mut self.mesh, draw.vertices(), draw.indices())?;
		self.prepared = Some(PreparedOverlay { key, rows });
		Ok(())
	}

	pub fn object(&self) -> Option<RenderObject<'_>> {
		let prepared = self.prepared.as_ref()?;
		(!prepared.rows.is_empty()).then(|| {
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
		self.text_scaled(x, y, text, color, 1.0);
	}

	fn text_scaled(&mut self, x: f32, y: f32, text: &str, color: [f32; 3], scale: f32) {
		for (i, c) in text.chars().enumerate() {
			let code = if c.is_ascii() {
				c as usize
			} else {
				b'?' as usize
			};
			let [u, v] = [(code % 16 * 8) as f32, (code / 16 * 8) as f32];
			self.quad(
				[
					x + i as f32 * COLUMN_WIDTH * scale,
					y,
					10.0 * scale,
					14.0 * scale,
				],
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

#[cfg(test)]
#[path = "../tests/unit/screen_overlay_layout.rs"]
mod screen_overlay_layout;
