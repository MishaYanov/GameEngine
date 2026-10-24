//! Public transport surface over the typed editor backend. Viewport-only DTOs stay here.
pub use engine_core::command::*;
pub use engine_core::events::{BackendEvent, BackendEventKind, ChangePage};
pub use engine_core::{BackendError, BackendErrorCode, CommandReceipt};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewportBoundsDto {
	pub x: f64,
	pub y: f64,
	pub width: f64,
	pub height: f64,
	pub visible: bool,
}
impl ViewportBoundsDto {
	pub fn validate(&self) -> Result<(), String> {
		if ![self.x, self.y, self.width, self.height]
			.iter()
			.all(|v| v.is_finite())
			|| self.x < 0.0
			|| self.y < 0.0
			|| self.width < 0.0
			|| self.height < 0.0
			|| self.width > 16384.0
			|| self.height > 16384.0
			|| self.x > 32768.0
			|| self.y > 32768.0
		{
			return Err(
				"Viewport bounds must be finite, nonnegative and within the supported client area"
					.into(),
			);
		}
		Ok(())
	}
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ViewportStatusDto {
	pub attached: bool,
	pub visible: bool,
	pub width: u32,
	pub height: u32,
	pub draw_calls: usize,
	pub instances: usize,
	pub triangles: usize,
	pub gpu_time_ms: Option<f64>,
	pub gpu_timestamps_supported: bool,
	pub error: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ViewportModifiersDto {
	pub shift: bool,
	pub control: bool,
	pub alt: bool,
	pub super_key: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewportInputDto {
	pub kind: String,
	pub position: Option<[f64; 2]>,
	pub extent: [u32; 2],
	pub button: Option<String>,
	pub code: Option<String>,
	pub repeat: bool,
	pub modifiers: ViewportModifiersDto,
	pub delta: Option<[f64; 2]>,
}
