//! Input-independent transport contracts. Scene payloads are validated by the domain.
use crate::project::ProjectData;
use crate::scene_catalog::SceneId;
use crate::scene_data::{CameraData, ColliderData, LightData, MaterialOverrideData, SceneData};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransformDto {
	pub position: [f32; 3],
	pub rotation: [f32; 3],
	pub scale: [f32; 3],
}
impl Default for TransformDto {
	fn default() -> Self {
		Self {
			position: [0.0; 3],
			rotation: [0.0; 3],
			scale: [1.0; 3],
		}
	}
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntitySpecDto {
	pub name: String,
	pub model_path: String,
	#[serde(default)]
	pub transform: TransformDto,
	#[serde(default)]
	pub parent: Option<String>,
	#[serde(default)]
	pub mesh_index: Option<usize>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransformEditDto {
	pub key: String,
	pub transform: TransformDto,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransformSpace {
	Local,
	World,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapSettingsDto {
	pub enabled: bool,
	pub translation: f32,
	pub rotation_radians: f32,
	pub scale: f32,
}
impl Default for SnapSettingsDto {
	fn default() -> Self {
		Self {
			enabled: false,
			translation: 0.25,
			rotation_radians: 15.0_f32.to_radians(),
			scale: 0.1,
		}
	}
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum EditorCommandDto {
	AddScene {
		id: SceneId,
		asset_path: String,
		document: SceneData,
	},
	SwitchScene {
		scene_id: SceneId,
	},
	RemoveScene {
		scene_id: SceneId,
	},
	SetStartupScene {
		scene_id: SceneId,
	},
	ChangeScenePath {
		scene_id: SceneId,
		asset_path: String,
	},
	PickSelection {
		pixel: [f64; 2],
		extent: [u32; 2],
		additive: bool,
	},
	Select {
		keys: Vec<String>,
	},
	Rename {
		key: String,
		name: String,
	},
	Spawn {
		entity: EntitySpecDto,
	},
	SetTransforms {
		edits: Vec<TransformEditDto>,
		space: TransformSpace,
		snap: bool,
	},
	TranslateSelection {
		delta: [f32; 3],
		snap: bool,
	},
	RotateSelection {
		radians: [f32; 3],
		space: TransformSpace,
		snap: bool,
	},
	ScaleSelection {
		factor: [f32; 3],
		snap: bool,
	},
	DuplicateSelection {
		offset: [f32; 3],
	},
	DeleteSelection,
	Reparent {
		key: String,
		parent: Option<String>,
		keep_world: bool,
	},
	SetMaterial {
		key: String,
		material: Option<MaterialOverrideData>,
	},
	SetCollider {
		key: String,
		collider: Option<ColliderData>,
	},
	UpsertLight {
		light: LightData,
	},
	RemoveLight {
		key: String,
	},
	SetCamera {
		camera: CameraData,
	},
	SetProject {
		project: ProjectData,
	},
	FrameSelection,
	OrbitCamera {
		yaw: f32,
		pitch: f32,
	},
	PanCamera {
		right: f32,
		up: f32,
	},
	DollyCamera {
		distance: f32,
	},
	SetSnap {
		settings: SnapSettingsDto,
	},
	BeginTransaction {
		label: String,
	},
	CommitTransaction,
	CancelTransaction,
	Undo,
	Redo,
	StartPlay,
	StopPlay,
	SetPaused {
		paused: bool,
	},
	SetPlayInput {
		movement: [f32; 3],
	},
	SetEntityInput {
		entity: String,
		movement: [f32; 3],
	},
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditorCommandRequest {
	pub expected_revision: u64,
	pub command: EditorCommandDto,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneStateDto {
	pub id: SceneId,
	pub asset_path: String,
	pub loaded: bool,
	pub dirty: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorStateDto {
	pub active_scene_id: SceneId,
	pub project_dirty: bool,
	pub scenes: Vec<SceneStateDto>,
	pub revision: u64,
	pub scene_revision: u64,
	pub selection: Vec<String>,
	pub mode: String,
	pub paused: bool,
	pub dirty: bool,
	pub can_undo: bool,
	pub can_redo: bool,
	pub transaction: Option<String>,
	pub snap: SnapSettingsDto,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorSnapshotDto {
	pub active_scene_id: SceneId,
	pub project_dirty: bool,
	pub scenes: Vec<SceneStateDto>,
	pub revision: u64,
	pub scene_revision: u64,
	pub document: SceneData,
	pub project: ProjectData,
	pub selection: Vec<String>,
	pub mode: String,
	pub paused: bool,
	pub dirty: bool,
	pub can_undo: bool,
	pub can_redo: bool,
	pub transaction: Option<String>,
	pub snap: SnapSettingsDto,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandDescriptorDto {
	pub name: String,
	pub label: String,
	pub category: String,
	pub undoable: bool,
	pub available_in_play: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorCapabilitiesDto {
	#[serde(default)]
	pub incremental_commands: bool,
	pub protocol_version: u32,
	pub scene_schema_version: u32,
	pub commands: Vec<CommandDescriptorDto>,
	pub max_selection: usize,
	pub max_history_entries: usize,
	pub history_budget_bytes: usize,
	pub embedded_viewport_supported: bool,
}
