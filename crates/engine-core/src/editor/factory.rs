//! Context-free command construction. Scene-dependent checks happen atomically
//! in EditorSession, so native callers and bridge deserialization share policy.
use std::{
	collections::{BTreeMap, HashSet},
	io::{self, Write},
};

use crate::camera_input::CameraIntent;
use crate::{
	EntityKey,
	command::{
		CommandDescriptorDto, EditorCommandDto, EditorCommandRequest, SceneBindingTarget,
		TransformDto,
	},
	scene_data::{CURRENT_SCHEMA_VERSION, CameraData, SceneData, validate_model_path},
};

pub const MAX_COMMAND_BYTES: usize = 4 * 1024 * 1024;
const MAX_TARGETS: usize = 4096;

pub struct CommandRequestBuilder {
	revision: u64,
	command: Option<EditorCommandDto>,
}
impl CommandRequestBuilder {
	pub fn command(mut self, command: EditorCommandDto) -> Self {
		self.command = Some(command);
		self
	}
	pub fn build(self) -> Result<EditorCommandRequest, String> {
		CommandFactory::build(
			self.revision,
			self.command.ok_or("A command builder needs an action")?,
		)
	}
}

/// Factory builds validated command intent; widgets and shortcuts use the same path.
pub struct CommandFactory;
impl CommandFactory {
	pub const MAX_PAYLOAD_BYTES: usize = MAX_COMMAND_BYTES;
	pub const MAX_ENTITY_TARGETS: usize = MAX_TARGETS;

	pub fn builder(expected_revision: u64) -> CommandRequestBuilder {
		CommandRequestBuilder {
			revision: expected_revision,
			command: None,
		}
	}
	pub fn build(
		expected_revision: u64,
		command: EditorCommandDto,
	) -> Result<EditorCommandRequest, String> {
		Self::validate(&command)?;
		Ok(EditorCommandRequest {
			expected_revision,
			command,
		})
	}
	pub fn validate(command: &EditorCommandDto) -> Result<(), String> {
		// Validate before a caller clones a document or allocates history. The
		// writer counts bytes without allocating another serialized command.
		serde_json::to_writer(CommandBudget(0), command)
			.map_err(|error| format!("Command size validation failed: {error}"))?;
		match command {
			EditorCommandDto::AddScene {
				id,
				asset_path,
				document,
			} => {
				crate::scene_catalog::SceneId::new(id.as_str())?;
				crate::scene_catalog::SceneDescriptor::new(id.clone(), asset_path)?;
				document.validate()?;
			}
			EditorCommandDto::SwitchScene { scene_id }
			| EditorCommandDto::RemoveScene { scene_id }
			| EditorCommandDto::SetStartupScene { scene_id } => {
				crate::scene_catalog::SceneId::new(scene_id.as_str())?;
			}
			EditorCommandDto::ChangeScenePath {
				scene_id,
				asset_path,
			} => {
				crate::scene_catalog::SceneDescriptor::new(scene_id.clone(), asset_path)?;
			}
			EditorCommandDto::Select { keys } => {
				target_count(keys.len())?;
				for key in keys {
					validate_key(key)?;
				}
			}
			EditorCommandDto::PickSelection { pixel, extent, .. } => {
				if extent.contains(&0)
					|| extent.iter().any(|axis| *axis > 16_384)
					|| !pixel.iter().all(|value| value.is_finite())
					|| pixel[0] < 0.0
					|| pixel[1] < 0.0
					|| pixel[0] >= extent[0] as f64
					|| pixel[1] >= extent[1] as f64
				{
					return Err(
						"Picking requires finite coordinates inside a bounded viewport".into(),
					);
				}
			}
			EditorCommandDto::Rename { key, name } => {
				validate_key(key)?;
				validate_name(name, 1024)?;
			}
			EditorCommandDto::Spawn { entity } => {
				validate_name(&entity.name, 1024)?;
				validate_model_path(&entity.model_path)?;
				transform(&entity.transform)?;
				if let Some(parent) = &entity.parent {
					validate_key(parent)?;
				}
				if entity.mesh_index.is_some_and(|index| index > 1_000_000) {
					return Err("Mesh index exceeds the supported limit".into());
				}
			}
			EditorCommandDto::SetTransforms { edits, .. } => {
				target_count(edits.len())?;
				let mut unique = HashSet::with_capacity(edits.len());
				for edit in edits {
					validate_key(&edit.key)?;
					transform(&edit.transform)?;
					if !unique.insert(edit.key.as_str()) {
						return Err("A transform command may target each entity only once".into());
					}
				}
			}
			EditorCommandDto::TranslateSelection { delta, .. }
			| EditorCommandDto::DuplicateSelection { offset: delta } => finite(delta, "Translation")?,
			EditorCommandDto::RotateSelection { radians, .. } => finite(radians, "Rotation")?,
			EditorCommandDto::ScaleSelection { factor, .. } => finite(factor, "Scale")?,
			EditorCommandDto::Reparent { key, parent, .. } => {
				validate_key(key)?;
				if let Some(parent) = parent {
					validate_key(parent)?;
				}
			}
			EditorCommandDto::SetMaterial { key, material } => {
				validate_key(key)?;
				if let Some(material) = material
					&& !material
						.base_color_factor
						.iter()
						.all(|value| value.is_finite() && (0.0..=1.0).contains(value))
				{
					return Err("Material color must be finite and within 0..=1".into());
				}
			}
			EditorCommandDto::SetCollider { key, collider } => {
				validate_key(key)?;
				if let Some(collider) = collider {
					collider.validate()?;
				}
			}
			EditorCommandDto::UpsertLight { light } => {
				// Reuse the authoritative schema validator for the typed light
				// payload; the active scene additionally checks key collisions.
				let camera = crate::Camera::default();
				SceneData {
					schema_version: CURRENT_SCHEMA_VERSION,
					bindings: BTreeMap::new(),
					entities: Vec::new(),
					camera: CameraData {
						follow: None,
						orbit: None,
						position: camera.position,
						yaw: camera.yaw,
						pitch: camera.pitch,
						vertical_fov: camera.vertical_fov,
						near_plane: camera.near,
						far_plane: camera.far,
					},
					lights: vec![light.clone()],
				}
				.validate()?;
			}
			EditorCommandDto::RemoveLight { key } => validate_key(key)?,
			EditorCommandDto::InstantiateLinkedPrefab {
				source,
				position,
				bindings,
			} => {
				source.validate()?;
				finite(position, "Prefab position")?;
				validate_slot_bindings(bindings)?;
			}
			EditorCommandDto::UpdateLinkedPrefab {
				root_key,
				source,
				bindings,
				..
			} => {
				validate_key(root_key)?;
				source.validate()?;
				if let Some(bindings) = bindings {
					validate_slot_bindings(bindings)?;
				}
			}
			EditorCommandDto::RevertLinkedPrefab { root_key, .. }
			| EditorCommandDto::UnpackLinkedPrefab { root_key } => validate_key(root_key)?,
			EditorCommandDto::AddComponent { key, component }
			| EditorCommandDto::ReplaceComponent { key, component } => {
				validate_key(key)?;
				component.validate()?;
			}
			EditorCommandDto::RemoveComponent { key, component_id }
			| EditorCommandDto::SetComponentEnabled {
				key, component_id, ..
			} => {
				validate_key(key)?;
				crate::components::ComponentId::new(component_id.as_str())?;
			}
			EditorCommandDto::SetCamera { camera } => camera.validate()?,
			EditorCommandDto::SetProject { project } => project.validate()?,
			EditorCommandDto::OrbitCamera { yaw, pitch } => {
				finite(&[*yaw, *pitch], "Camera orbit")?
			}
			EditorCommandDto::PanCamera { right, up } => finite(&[*right, *up], "Camera pan")?,
			EditorCommandDto::DollyCamera { distance } => finite(&[*distance], "Camera dolly")?,
			EditorCommandDto::SetSnap { settings } => {
				if ![
					settings.translation,
					settings.rotation_radians,
					settings.scale,
				]
				.iter()
				.all(|value| value.is_finite() && *value > 0.0 && *value <= 1.0e6)
				{
					return Err("Snap increments must be finite and within 0..=1000000".into());
				}
			}
			EditorCommandDto::BeginTransaction { label } => validate_name(label, 256)?,
			EditorCommandDto::SetPlayInput { movement } => finite(movement, "Movement")?,
			EditorCommandDto::SetCharacterInput {
				key, movement_xz, ..
			} => {
				validate_key(key)?;
				finite(movement_xz, "Character movement")?;
			}
			EditorCommandDto::SetEntityInput { entity, movement } => {
				validate_key(entity)?;
				finite(movement, "Movement")?;
			}
			EditorCommandDto::SetSceneBinding {
				name,
				scene_id,
				target,
			} => {
				crate::scene_catalog::SceneId::new(scene_id.as_str())?;
				validate_name(name, 1024)?;

				match target {
					SceneBindingTarget::Assign { key } => {
						validate_key(key)?;
					}
					SceneBindingTarget::Clear {} => {}
				}
			}
			EditorCommandDto::SetPlayCameraInput {
				workspace_generation,
				yaw_delta,
				pitch_delta,
				log_distance_delta,
				run_id,
				scene_generation,
			} => {
				validate_name(run_id, 128)?;
				if (*scene_generation).max(*workspace_generation) > 9_007_199_254_740_991 {
					return Err(
						"Camera input generation exceeds the exact JSON integer range".into(),
					);
				}
				CameraIntent {
					yaw_delta: *yaw_delta,
					pitch_delta: *pitch_delta,
					log_distance_delta: *log_distance_delta,
				}
				.validate()?;
			}
			EditorCommandDto::DeleteSelection
			| EditorCommandDto::FrameSelection
			| EditorCommandDto::CommitTransaction
			| EditorCommandDto::CancelTransaction
			| EditorCommandDto::Undo
			| EditorCommandDto::Redo
			| EditorCommandDto::StartPlay
			| EditorCommandDto::StopPlay
			| EditorCommandDto::SetPaused { .. } => {}
		}
		Ok(())
	}

	pub fn capabilities() -> Vec<CommandDescriptorDto> {
		[
			("add_scene", "Add scene", "scene", true, false),
			("switch_scene", "Switch scene", "scene", false, false),
			("remove_scene", "Remove scene", "scene", true, false),
			(
				"set_startup_scene",
				"Set startup scene",
				"scene",
				true,
				false,
			),
			(
				"change_scene_path",
				"Change scene path",
				"scene",
				true,
				false,
			),
			("set_project", "Configure project", "project", true, false),
			("add_component", "Add component", "component", true, false),
			(
				"replace_component",
				"Edit component",
				"component",
				true,
				false,
			),
			(
				"remove_component",
				"Remove component",
				"component",
				true,
				false,
			),
			(
				"set_component_enabled",
				"Enable component",
				"component",
				true,
				false,
			),
			(
				"instantiate_linked_prefab",
				"Instantiate linked prefab",
				"prefab",
				true,
				false,
			),
			(
				"update_linked_prefab",
				"Update linked prefab",
				"prefab",
				true,
				false,
			),
			(
				"revert_linked_prefab",
				"Revert linked overrides",
				"prefab",
				true,
				false,
			),
			(
				"unpack_linked_prefab",
				"Unpack linked prefab",
				"prefab",
				true,
				false,
			),
			(
				"set_entity_input",
				"Set controlled entity input",
				"simulation",
				false,
				true,
			),
			("select", "Select entities", "selection", false, true),
			("pick_selection", "Pick entity", "selection", false, false),
			("rename", "Rename", "entity", true, false),
			("spawn", "Create entity", "entity", true, false),
			("set_transforms", "Set transforms", "transform", true, false),
			(
				"translate_selection",
				"Move selection",
				"transform",
				true,
				false,
			),
			(
				"rotate_selection",
				"Rotate selection",
				"transform",
				true,
				false,
			),
			(
				"scale_selection",
				"Scale selection",
				"transform",
				true,
				false,
			),
			(
				"duplicate_selection",
				"Duplicate group",
				"entity",
				true,
				false,
			),
			("delete_selection", "Delete group", "entity", true, false),
			("reparent", "Change parent", "hierarchy", true, false),
			(
				"set_material",
				"Set material override",
				"material",
				true,
				false,
			),
			("set_collider", "Set collider", "collision", true, false),
			(
				"set_scene_binding",
				"Set scene binding",
				"scene",
				true,
				false,
			),
			("upsert_light", "Set light", "lighting", true, false),
			("remove_light", "Remove light", "lighting", true, false),
			("set_camera", "Set camera", "camera", true, false),
			("frame_selection", "Frame selection", "camera", true, false),
			("orbit_camera", "Orbit", "camera", true, false),
			("pan_camera", "Pan", "camera", true, false),
			("dolly_camera", "Dolly", "camera", true, false),
			("set_snap", "Configure snapping", "settings", false, false),
			(
				"begin_transaction",
				"Begin grouped edit",
				"history",
				false,
				false,
			),
			(
				"commit_transaction",
				"Commit grouped edit",
				"history",
				true,
				false,
			),
			(
				"cancel_transaction",
				"Cancel grouped edit",
				"history",
				false,
				false,
			),
			("undo", "Undo", "history", false, false),
			("redo", "Redo", "history", false, false),
			("start_play", "Play", "simulation", false, false),
			("stop_play", "Stop", "simulation", false, true),
			("set_paused", "Pause simulation", "simulation", false, true),
			(
				"set_play_camera_input",
				"Set orbit camera input",
				"simulation",
				false,
				true,
			),
			(
				"set_character_input",
				"Set character movement and jump intent",
				"simulation",
				false,
				true,
			),
			(
				"set_play_input",
				"Set movement intent",
				"simulation",
				false,
				true,
			),
		]
		.into_iter()
		.map(
			|(name, label, category, undoable, available_in_play)| CommandDescriptorDto {
				name: name.into(),
				label: label.into(),
				category: category.into(),
				undoable,
				available_in_play,
			},
		)
		.collect()
	}
}

fn target_count(count: usize) -> Result<(), String> {
	if count > MAX_TARGETS {
		return Err("A command may target at most 4096 entities".into());
	}
	Ok(())
}
fn validate_slot_bindings(bindings: &BTreeMap<String, crate::EntityKey>) -> Result<(), String> {
	if bindings.len() > 32 {
		return Err("Prefab reference slot binding count exceeds 32".into());
	}
	for (name, key) in bindings {
		if name.is_empty()
			|| name.len() > 64
			|| !name
				.bytes()
				.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
		{
			return Err("Invalid prefab reference slot name".into());
		}
		validate_key(key.as_str())?;
	}
	Ok(())
}
fn validate_key(key: &str) -> Result<(), String> {
	if key.len() > 256 || key.chars().any(char::is_control) {
		return Err("Entity keys must contain at most 256 bytes and no control characters".into());
	}
	EntityKey::new(key).map(|_| ())
}
fn validate_name(name: &str, max: usize) -> Result<(), String> {
	if name.trim().is_empty() || name.len() > max || name.chars().any(char::is_control) {
		return Err(format!(
			"Name or label must contain visible text, no control characters, and at most {max} bytes"
		));
	}
	Ok(())
}
fn transform(value: &TransformDto) -> Result<(), String> {
	crate::Transform {
		position: value.position,
		rotation: value.rotation,
		scale: value.scale,
	}
	.validate()
}
fn finite(values: &[f32], context: &str) -> Result<(), String> {
	if values.iter().any(|value| !value.is_finite()) {
		return Err(format!("{context} must contain only finite values"));
	}
	Ok(())
}
struct CommandBudget(usize);
impl Write for CommandBudget {
	fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
		self.0 = self
			.0
			.checked_add(bytes.len())
			.filter(|size| *size <= MAX_COMMAND_BYTES)
			.ok_or_else(|| io::Error::other("Command exceeds the 4 MiB payload limit"))?;
		Ok(bytes.len())
	}
	fn flush(&mut self) -> io::Result<()> {
		Ok(())
	}
}
