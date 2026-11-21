//! Editor-only view/tool contracts. These are never serialized into a game scene.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewportTool {
	Hand,
	#[default]
	Move,
	Rotate,
	Scale,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewportSpace {
	#[default]
	World,
	Local,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewportPivot {
	#[default]
	Pivot,
	Center,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewportProjection {
	#[default]
	Perspective,
	Orthographic,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewportPlane {
	#[default]
	Xz,
	Xy,
	Yz,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewportView {
	#[default]
	Scene,
	Game,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewportOrientation {
	#[default]
	Free,
	Top,
	Bottom,
	Front,
	Back,
	Left,
	Right,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewportSettingsDto {
	pub tool: ViewportTool,
	pub space: ViewportSpace,
	pub pivot: ViewportPivot,
	pub grid: bool,
	#[serde(default)]
	pub colliders: bool,
	pub grid_spacing: f32,
	pub snap: bool,
	pub move_snap: f32,
	pub rotation_snap_degrees: f32,
	pub scale_snap: f32,
	pub projection: ViewportProjection,
	pub plane: ViewportPlane,
	pub view: ViewportView,
}
impl Default for ViewportSettingsDto {
	fn default() -> Self {
		Self {
			tool: ViewportTool::Move,
			space: ViewportSpace::World,
			pivot: ViewportPivot::Pivot,
			grid: true,
			colliders: false,
			grid_spacing: 1.0,
			snap: false,
			move_snap: 1.0,
			rotation_snap_degrees: 15.0,
			scale_snap: 0.1,
			projection: ViewportProjection::Perspective,
			plane: ViewportPlane::Xz,
			view: ViewportView::Scene,
		}
	}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewportPlacementDto {
	pub path: String,
	pub position: Option<[f32; 3]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewportTransformPreviewDto {
	pub key: String,
	pub transform: crate::editor::TransformDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewportEditorStateDto {
	pub settings: ViewportSettingsDto,
	pub camera_position: [f32; 3],
	pub camera_target: [f32; 3],
	pub camera_distance: f32,
	pub revision: u64,
	pub workspace_generation: Option<u64>,
	pub gesture: Option<String>,
	pub placement: Option<ViewportPlacementDto>,
	pub focused: bool,
	pub error: Option<String>,
	#[serde(default)]
	pub collider_error: Option<String>,
	pub transform_preview: Option<ViewportTransformPreviewDto>,
	#[serde(default)]
	pub game_status: Option<crate::editor::PlayStatusDto>,
	#[serde(default)]
	pub orbit_status: Option<OrbitStatusDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrbitStatusDto {
	pub blocker: Option<String>,
	pub recovering: bool,
	pub target_lost: bool,
}
impl Default for ViewportEditorStateDto {
	fn default() -> Self {
		Self {
			settings: ViewportSettingsDto::default(),
			camera_position: [8.0, 6.0, 8.0],
			camera_target: [0.0; 3],
			camera_distance: 12.0,
			revision: 0,
			workspace_generation: None,
			gesture: None,
			placement: None,
			focused: false,
			error: None,
			collider_error: None,
			transform_preview: None,
			game_status: None,
			orbit_status: None,
		}
	}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ViewportActionDto {
	RestartRun {
		run_id: String,
		expected_revision: u64,
	},
	SetTool {
		tool: ViewportTool,
	},
	SetSpace {
		space: ViewportSpace,
	},
	SetPivot {
		pivot: ViewportPivot,
	},
	SetGrid {
		visible: bool,
	},
	SetColliders {
		visible: bool,
	},
	SetGridSpacing {
		spacing: f32,
	},
	SetSnap {
		enabled: bool,
	},
	SetSnapIncrements {
		translation: f32,
		rotation_degrees: f32,
		scale: f32,
	},
	SetProjection {
		projection: ViewportProjection,
	},
	SetPlane {
		plane: ViewportPlane,
	},
	SetView {
		view: ViewportView,
	},
	Orient {
		orientation: ViewportOrientation,
	},
	Zoom {
		factor: f32,
	},
	FrameSelection,
	ResetView,
	Cancel,
	BeginPlacement {
		path: String,
	},
	UpdatePlacement {
		normalized: Option<[f64; 2]>,
	},
	CommitPlacement,
	CancelPlacement,
}
impl ViewportActionDto {
	pub fn validate(&self) -> Result<(), String> {
		let positive = |value: f32| value.is_finite() && (0.0001..=1_000_000.0).contains(&value);
		match self {
			Self::RestartRun { run_id, .. }
				if run_id.is_empty() || run_id.len() > 64 || !run_id.is_ascii() =>
			{
				Err("Restart requires a valid run identity".into())
			}
			Self::SetGridSpacing { spacing } if !positive(*spacing) => {
				Err("Grid spacing must be finite and positive".into())
			}
			Self::SetSnapIncrements {
				translation,
				rotation_degrees,
				scale,
			} if !positive(*translation) || !positive(*rotation_degrees) || !positive(*scale) => {
				Err("Snap increments must be finite and positive".into())
			}
			Self::Zoom { factor } if !factor.is_finite() || !(0.01..=100.0).contains(factor) => {
				Err("Zoom factor must be between 0.01 and 100".into())
			}
			Self::BeginPlacement { path } => engine_core::scene_data::validate_model_path(path),
			Self::UpdatePlacement {
				normalized: Some(point),
			} if !point
				.iter()
				.all(|value| value.is_finite() && (0.0..=1.0).contains(value)) =>
			{
				Err("Placement coordinates must be normalized inside the viewport".into())
			}
			_ => Ok(()),
		}
	}
}
