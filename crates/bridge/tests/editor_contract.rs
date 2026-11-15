use bridge::editor::{EditorCommandDto, EditorCommandRequest, ViewportBoundsDto};
use serde_json::json;

#[test]
fn runtime_status_and_restart_use_separate_play_and_run_identities() {
	use bridge::editor::{GameplayStatus, PlayStatusDto};
	use bridge::viewport::{ViewportActionDto, ViewportEditorStateDto};
	let mut state = ViewportEditorStateDto::default();
	state.game_status = Some(PlayStatusDto {
		play_session_id: "play-1".into(),
		run_id: "run-2".into(),
		scene_id: engine_core::scene_catalog::SceneId::new("finish").unwrap(),
		paused: false,
		pending_scene_id: None,
		gameplay: Some(GameplayStatus {
			objective: "Run complete!".into(),
			detail: None,
			score: Some(1),
			completed: true,
		}),
	});
	let wire = serde_json::to_value(&state).unwrap();
	assert_eq!(wire["game_status"]["play_session_id"], "play-1");
	assert_eq!(wire["game_status"]["run_id"], "run-2");
	let decoded: ViewportEditorStateDto = serde_json::from_value(wire.clone()).unwrap();
	assert_eq!(decoded.game_status, state.game_status);
	let mut legacy = wire;
	legacy.as_object_mut().unwrap().remove("game_status");
	assert!(
		serde_json::from_value::<ViewportEditorStateDto>(legacy)
			.unwrap()
			.game_status
			.is_none()
	);
	let action = json!({"type":"restart_run","run_id":"run-2","expected_revision":17});
	let decoded: ViewportActionDto = serde_json::from_value(action.clone()).unwrap();
	decoded.validate().unwrap();
	assert_eq!(serde_json::to_value(decoded).unwrap(), action);
	for invalid in [
		json!({"type":"restart_run","run_id":"run-2"}),
		json!({"type":"restart_run","run_id":"run-2","expected_revision":17,"scene_id":"finish"}),
	] {
		assert!(serde_json::from_value::<ViewportActionDto>(invalid).is_err());
	}
	assert!(
		ViewportActionDto::RestartRun {
			run_id: "x".repeat(65),
			expected_revision: 17
		}
		.validate()
		.is_err()
	);
}

#[test]
fn typed_editor_command_roundtrip_uses_explicit_revision_tag_and_strict_nested_fields() {
	let value = json!({"expected_revision":42,"command":{"type":"set_transforms","edits":[{"key":"robot","transform":{"position":[1,2,3],"rotation":[0,0,0],"scale":[1,1,1]}}],"space":"world","snap":true}});
	let request: EditorCommandRequest = serde_json::from_value(value.clone()).unwrap();
	assert_eq!(request.expected_revision, 42);
	assert!(matches!(
		request.command,
		EditorCommandDto::SetTransforms { .. }
	));
	let decoded: EditorCommandRequest =
		serde_json::from_value(serde_json::to_value(&request).unwrap()).unwrap();
	assert_eq!(decoded.expected_revision, request.expected_revision);
	assert_eq!(decoded.command, request.command);
	for pointer in [
		"",
		"/command",
		"/command/edits/0",
		"/command/edits/0/transform",
	] {
		let mut invalid = value.clone();
		invalid
			.pointer_mut(pointer)
			.unwrap()
			.as_object_mut()
			.unwrap()
			.insert("unexpected".into(), json!(true));
		assert!(
			serde_json::from_value::<EditorCommandRequest>(invalid).is_err(),
			"accepted extra field at {pointer}"
		);
	}
	let mut missing = value;
	missing.as_object_mut().unwrap().remove("expected_revision");
	assert!(serde_json::from_value::<EditorCommandRequest>(missing).is_err());
}

#[test]
fn spawn_defaults_are_transport_defaults_and_unknown_command_or_space_is_rejected() {
	let request: EditorCommandRequest = serde_json::from_value(json!({"expected_revision":0,"command":{"type":"spawn","entity":{"name":"New","model_path":"props/robot.glb"}}})).unwrap();
	let EditorCommandDto::Spawn { entity } = request.command else {
		panic!("wrong variant")
	};
	assert_eq!(entity.transform.scale, [1.0; 3]);
	assert_eq!(entity.transform.position, [0.0; 3]);
	assert!(entity.parent.is_none());
	assert!(entity.mesh_index.is_none());
	for command in [
		json!({"type":"execute_code","source":"arbitrary"}),
		json!({"type":"set_transforms","edits":[],"space":"screen","snap":false}),
	] {
		assert!(
			serde_json::from_value::<EditorCommandRequest>(
				json!({"expected_revision":0,"command":command})
			)
			.is_err()
		);
	}
}

#[test]
fn viewport_bounds_accept_hidden_zero_size_and_reject_nonfinite_negative_or_excessive_geometry() {
	assert!(ViewportBoundsDto::default().validate().is_ok());
	assert!(
		ViewportBoundsDto {
			x: 32768.0,
			y: 32768.0,
			width: 16384.0,
			height: 16384.0,
			visible: true
		}
		.validate()
		.is_ok()
	);
	for field in 0..4 {
		for value in [-1.0, f64::NAN, f64::INFINITY, 65536.0] {
			let mut bounds = ViewportBoundsDto::default();
			match field {
				0 => bounds.x = value,
				1 => bounds.y = value,
				2 => bounds.width = value,
				_ => bounds.height = value,
			}
			assert!(bounds.validate().is_err(), "accepted field {field}={value}");
		}
	}
}

#[test]
fn typed_component_commands_reject_unknown_fields_and_malformed_component_shapes_at_transport_boundary()
 {
	for (command, pointer) in [
		(
			json!({"type":"set_material","key":"a","material":{"base_color_factor":[0.1,0.2,0.3,1.0]}}),
			"/material",
		),
		(
			json!({"type":"set_collider","key":"a","collider":{"kind":"sphere","radius":1.0,"offset":[0.0,0.0,0.0]}}),
			"/collider",
		),
		(
			json!({"type":"upsert_light","light":{"key":"sun","name":"Sun","kind":"directional","position":[0,0,0],"direction":[0,-1,0],"color":[1,1,1],"intensity":1,"range":10}}),
			"/light",
		),
		(
			json!({"type":"set_camera","camera":{"position":[0,0,3],"yaw":0,"pitch":0,"vertical_fov":1,"near":0.1,"far":100}}),
			"/camera",
		),
	] {
		let valid = json!({"expected_revision":0,"command":command});
		let parsed: EditorCommandRequest = serde_json::from_value(valid.clone()).unwrap();
		let reloaded: EditorCommandRequest =
			serde_json::from_value(serde_json::to_value(&parsed).unwrap()).unwrap();
		assert_eq!(reloaded.command, parsed.command);
		let mut unknown = valid;
		unknown["command"]
			.pointer_mut(pointer)
			.unwrap()
			.as_object_mut()
			.unwrap()
			.insert("unexpected".into(), json!(true));
		assert!(
			serde_json::from_value::<EditorCommandRequest>(unknown).is_err(),
			"accepted extra component field at {pointer}"
		);
	}
	for command in [
		json!({"type":"set_material","key":"a","material":{"base_color_factor":[1,1,1]}}),
		json!({"type":"set_collider","key":"a","collider":{"kind":"sphere","radius":"large","offset":[0,0,0]}}),
		json!({"type":"set_camera","camera":{"position":[0,0],"yaw":0,"pitch":0,"vertical_fov":1,"near":0.1,"far":100}}),
	] {
		assert!(
			serde_json::from_value::<EditorCommandRequest>(
				json!({"expected_revision":0,"command":command})
			)
			.is_err()
		);
	}
}

#[test]
fn scene_lifecycle_commands_roundtrip_stable_ids_and_reject_extra_transport_fields() {
	let document = json!({
		"schema_version":3,"entities":[],"bindings":{},"lights":[],
		"camera":{"position":[0,0,3],"yaw":0,"pitch":0,"vertical_fov":1,"near":0.1,"far":100}
	});
	for command in [
		json!({"type":"add_scene","id":"secondary","asset_path":"scenes/secondary.json","document":document}),
		json!({"type":"switch_scene","scene_id":"secondary"}),
		json!({"type":"remove_scene","scene_id":"secondary"}),
		json!({"type":"set_startup_scene","scene_id":"secondary"}),
		json!({"type":"change_scene_path","scene_id":"secondary","asset_path":"scenes/renamed.json"}),
	] {
		let valid = json!({"expected_revision":42,"command":command});
		let parsed: EditorCommandRequest = serde_json::from_value(valid.clone()).unwrap();
		let encoded = serde_json::to_value(&parsed).unwrap();
		let id_field = if command["type"] == "add_scene" {
			"id"
		} else {
			"scene_id"
		};
		assert_eq!(encoded["command"][id_field], "secondary");
		let decoded: EditorCommandRequest = serde_json::from_value(encoded).unwrap();
		assert_eq!(decoded.command, parsed.command);
		assert_eq!(decoded.expected_revision, 42);
		let mut extra = valid;
		extra["command"]
			.as_object_mut()
			.unwrap()
			.insert("unexpected".into(), json!(true));
		assert!(serde_json::from_value::<EditorCommandRequest>(extra).is_err());
	}
	for command in [
		json!({"type":"switch_scene"}),
		json!({"type":"switch_scene","scene_id":42}),
		json!({"type":"add_scene","id":"secondary","asset_path":"scenes/secondary.json"}),
		json!({"type":"change_scene_path","scene_id":"secondary"}),
		json!({"type":"change_scene_path","scene_id":"two words","asset_path":"scenes/renamed.json"}),
		json!({"type":"set_startup_scene","scene_id":""}),
	] {
		assert!(
			serde_json::from_value::<EditorCommandRequest>(
				json!({"expected_revision":42,"command":command})
			)
			.is_err()
		);
	}
}

#[test]
fn scene_binding_commands_roundtrip_assign_and_clear_and_reject_invalid_targets() {
	let request = |target| {
		json!({
			"expected_revision": 1,
			"command": {
				"type": "set_scene_binding",
				"scene_id": "main",
				"name": "player",
				"target": target,
			},
		})
	};
	for target in [
		json!({"kind": "assign", "key": "a"}),
		json!({"kind": "clear"}),
	] {
		let value = request(target);
		let decoded: EditorCommandRequest = serde_json::from_value(value.clone()).unwrap();
		assert_eq!(serde_json::to_value(decoded).unwrap(), value);
	}

	for target in [
		json!(null),
		json!({"key": "a"}),
		json!({"kind": "assign"}),
		json!({"kind": "assign", "key": null}),
		json!({"kind": "assign", "key": "a", "extra": true}),
		json!({"kind": "clear", "key": "a"}),
		json!({"kind": "replace", "key": "a"}),
	] {
		let value = request(target);
		assert!(
			serde_json::from_value::<EditorCommandRequest>(value.clone()).is_err(),
			"accepted malformed scene binding request {value}"
		);
	}
	let mut missing_target = request(json!(null));
	missing_target["command"]
		.as_object_mut()
		.unwrap()
		.remove("target");
	assert!(serde_json::from_value::<EditorCommandRequest>(missing_target).is_err());
}
