//! Input-independent transport contracts. Scene payloads are validated by the domain.
use crate::EntityKey;
use crate::project::ProjectData;
use crate::scene_catalog::SceneId;
use crate::scene_data::{
	CameraData, ColliderData, FolderId, LightData, LightKind, MaterialOverrideData, SceneData,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_COLLIDER_UPDATES: usize = 3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColliderUpdateDto {
	pub key: String,
	pub collider: ColliderData,
}

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
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub asset_id: Option<crate::asset_catalog::AssetId>,
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
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SceneBindingTarget {
	Assign { key: String },
	Clear {},
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
	CreateCamera,
	CreatePlayer,
	CreateAudioSource,
	CreateSky,
	CreateEmpty,
	CreateLight {
		light_kind: LightKind,
	},
	CreateParticleEmitter,
	SetActiveSky {
		key: Option<EntityKey>,
	},
	SetSceneEnvironment {
		ambient_linear: [f32; 3],
		sky_exposure_ev: f32,
	},
	SetLocalPlayer {
		key: Option<EntityKey>,
	},
	AlignCameraToSceneView {
		key: EntityKey,
		position: [f32; 3],
		yaw: f32,
		pitch: f32,
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
	ReparentObjects {
		keys: Vec<EntityKey>,
		parent: Option<EntityKey>,
		keep_world: bool,
	},
	CreateFolder {
		name: String,
		parent: Option<FolderId>,
	},
	RenameFolder {
		id: FolderId,
		name: String,
	},
	MoveFolder {
		id: FolderId,
		parent: Option<FolderId>,
	},
	MoveRootsToFolder {
		roots: Vec<EntityKey>,
		folder: Option<FolderId>,
	},
	RemoveFolder {
		id: FolderId,
	},
	SetMaterial {
		key: String,
		material: Option<MaterialOverrideData>,
	},
	SetCollider {
		key: String,
		collider: Option<ColliderData>,
	},
	SetColliders {
		updates: Vec<ColliderUpdateDto>,
	},
	AddComponent {
		key: String,
		component: crate::components::ComponentRecord,
	},
	ReplaceComponent {
		key: String,
		component: crate::components::ComponentRecord,
	},
	RemoveComponent {
		key: String,
		component_id: crate::components::ComponentId,
	},
	SetComponentEnabled {
		key: String,
		component_id: crate::components::ComponentId,
		enabled: bool,
	},
	InstantiateLinkedPrefab {
		source: crate::linked_prefab::LinkedPrefabSource,
		position: [f32; 3],
		#[serde(default)]
		bindings: BTreeMap<String, crate::EntityKey>,
	},
	UpdateLinkedPrefab {
		root_key: String,
		source: crate::linked_prefab::LinkedPrefabSource,
		resolution: crate::linked_prefab::PrefabConflictResolution,
		#[serde(default)]
		bindings: Option<BTreeMap<String, crate::EntityKey>>,
	},
	UpdateLinkedPrefabReviewed {
		root_key: String,
		source: crate::linked_prefab::LinkedPrefabSource,
		#[serde(default)]
		bindings: Option<BTreeMap<String, crate::EntityKey>>,
		resolutions: Vec<crate::linked_prefab::LinkedPrefabOverrideResolution>,
	},
	RevertLinkedPrefab {
		root_key: String,
		property: Option<crate::linked_prefab::LinkedPrefabOverrideTarget>,
	},
	UnpackLinkedPrefab {
		root_key: String,
	},
	UnpackLinkedPrefabOuter {
		root_key: String,
	},
	DuplicateLinkedPrefab {
		root_key: String,
	},
	RelinkLinkedPrefab {
		root_key: String,
		source: crate::linked_prefab::LinkedPrefabSource,
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
	SetDefaultGameCamera {
		key: EntityKey,
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
	SetCharacterInput {
		key: String,
		movement_xz: [f32; 2],
		jump_held: bool,
	},
	SetEntityInput {
		entity: String,
		movement: [f32; 3],
	},
	SetSceneBinding {
		scene_id: SceneId,
		name: String,
		target: SceneBindingTarget,
	},
	SetPlayCameraInput {
		workspace_generation: u64,
		run_id: String,
		scene_generation: u64,
		yaw_delta: f64,
		pitch_delta: f64,
		log_distance_delta: f64,
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
pub use crate::simulation_session::GameplayStatus;

/// Runtime-only state. These IDs are deliberately independent of authored scenes
/// and revisions, and a new run retains the enclosing Play session ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayStatusDto {
	pub play_session_id: String,
	pub run_id: String,
	#[serde(default)]
	pub scene_generation: u64,
	pub scene_id: SceneId,
	pub paused: bool,
	#[serde(default)]
	pub pending_scene_id: Option<SceneId>,
	#[serde(default)]
	pub gameplay: Option<GameplayStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorStateDto {
	pub active_scene_id: SceneId,
	#[serde(default)]
	pub play_session_id: Option<String>,
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
	#[serde(default)]
	pub play_session_id: Option<String>,
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameplayRoleDescriptorDto {
	pub name: String,
	pub label: String,
	pub requirement_hint: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameplayModuleDescriptorDto {
	pub id: String,
	pub label: String,
	pub description: String,
	pub default_roles: Vec<GameplayRoleDescriptorDto>,
	pub scene_roles: BTreeMap<String, Vec<GameplayRoleDescriptorDto>>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorCapabilitiesDto {
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub state_machines: Option<crate::state_machine::MachineDescriptor>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub component_types: Vec<crate::components::ComponentTypeDescriptor>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub object_scripts: Vec<crate::components::ScriptDescriptor>,
	#[serde(default)]
	pub incremental_commands: bool,
	#[serde(default)]
	pub gameplay_modules: Vec<GameplayModuleDescriptorDto>,
	pub protocol_version: u32,
	pub scene_schema_version: u32,
	pub commands: Vec<CommandDescriptorDto>,
	pub max_selection: usize,
	pub max_history_entries: usize,
	pub history_budget_bytes: usize,
	pub embedded_viewport_supported: bool,
	#[serde(default, skip_serializing_if = "std::ops::Not::not")]
	pub scene_warming_supported: bool,
	#[serde(default, skip_serializing_if = "std::ops::Not::not")]
	pub scene_verification_supported: bool,
	#[serde(default, skip_serializing_if = "std::ops::Not::not")]
	pub input_action_assets: bool,
	#[serde(default, skip_serializing_if = "std::ops::Not::not")]
	pub animator_assets: bool,
	#[serde(default, skip_serializing_if = "std::ops::Not::not")]
	pub controls_graph_assets: bool,
	#[serde(default, skip_serializing_if = "std::ops::Not::not")]
	pub sky_panorama_assets: bool,
}
