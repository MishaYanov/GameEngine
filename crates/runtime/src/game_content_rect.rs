//! Physical pixel bounds shared by Game projection, drawing and absolute input.
use engine_core::game_camera::MAX_GAME_ASPECT;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameContentRect {
	x: u32,
	width: u32,
	height: u32,
}

impl GameContentRect {
	/// Center the widest view supported by the camera's obstruction envelope.
	pub fn fit(width: u32, height: u32) -> Option<Self> {
		if width == 0 || height == 0 || width > i32::MAX as u32 || height > i32::MAX as u32 {
			return None;
		}
		let content_width = f64::from(width).min(f64::from(height) * MAX_GAME_ASPECT) as u32;
		Some(Self {
			x: (width - content_width) / 2,
			width: content_width,
			height,
		})
	}

	pub fn extent(self) -> [u32; 2] {
		[self.width, self.height]
	}

	/// Physical viewport and scissor, in x, y, width, height order.
	pub fn pixels(self) -> [u32; 4] {
		[self.x, 0, self.width, self.height]
	}

	pub fn aspect(self) -> f32 {
		(self.width as f32 / self.height as f32).min(MAX_GAME_ASPECT as f32)
	}

	/// Convert an absolute framebuffer position to content pixels. Bars are outside Game input.
	pub fn local_pointer(self, [x, y]: [f64; 2]) -> Option<[f64; 2]> {
		let x = x - f64::from(self.x);
		((0.0..f64::from(self.width)).contains(&x) && (0.0..f64::from(self.height)).contains(&y))
			.then_some([x, y])
	}
}
