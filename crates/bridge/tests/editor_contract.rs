use bridge::editor::{EditorCommandDto, EditorCommandRequest, ViewportBoundsDto};
use serde_json::json;

#[test]
fn gameplay_module_catalog_roundtrips_scene_overrides_and_accepts_legacy_capabilities() {
	use bridge::editor::EditorCapabilitiesDto;
	let legacy = json!({
		"incremental_commands": true,
		"protocol_version": 2,
		"scene_schema_version": 3,
		"commands": [],
		"max_selection": 4096,
		"max_history_entries": 128,
		"history_budget_bytes": 16777216,
		"embedded_viewport_supported": true
	});
	let old: EditorCapabilitiesDto = serde_json::from_value(legacy.clone()).unwrap();
	assert!(old.gameplay_modules.is_empty());
	let mut value = legacy;
	value["gameplay_modules"] = json!([{
		"id": "example_game",
		"label": "Example game",
		"description": "Module metadata is independent of Courtyard.",
		"default_roles": [{
			"name": "actor", "label": "Actor", "requirement_hint": "Choose a moving entity."
		}],
		"scene_roles": {
			"menu": [],
			"ending": [{"name": "goal", "label": "Goal", "requirement_hint": "Required here."}]
		}
	}]);
	let decoded: EditorCapabilitiesDto = serde_json::from_value(value.clone()).unwrap();
	assert_eq!(decoded.gameplay_modules[0].default_roles[0].name, "actor");
	assert!(decoded.gameplay_modules[0].scene_roles["menu"].is_empty());
	assert_eq!(serde_json::to_value(decoded).unwrap(), value);
	value["gameplay_modules"][0]["scene_roles"]["ending"] = json!("invalid");
	assert!(serde_json::from_value::<EditorCapabilitiesDto>(value).is_err());
}

#[test]
fn runtime_status_and_restart_use_separate_play_and_run_identities() {
	use bridge::editor::{GameplayStatus, PlayStatusDto};
	use bridge::viewport::{ViewportActionDto, ViewportEditorStateDto};
	let state = ViewportEditorStateDto {
		game_status: Some(PlayStatusDto {
			play_session_id: "play-1".into(),
			run_id: "run-2".into(),
			scene_generation: 1,
			scene_id: engine_core::scene_catalog::SceneId::new("finish").unwrap(),
			paused: false,
			pending_scene_id: None,
			gameplay: Some(GameplayStatus {
				objective: "Run complete!".into(),
				detail: None,
				score: Some(1),
				completed: true,
			}),
		}),
		..Default::default()
	};
	let wire = serde_json::to_value(&state).unwrap();
	assert_eq!(wire["game_status"]["play_session_id"], "play-1");
	assert_eq!(wire["game_status"]["run_id"], "run-2");
	assert_eq!(wire["game_status"]["scene_generation"], 1);
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

#[test]
fn collider_box_sphere_and_removal_roundtrip_and_reject_malformed_shapes() {
	let request = |collider| {
		json!({
			"expected_revision": 7,
			"command": {"type": "set_collider", "key": "crate", "collider": collider},
		})
	};
	for collider in [
		json!({"kind": "box", "half_extents": [0.5, 1.0, 2.0], "offset": [-1.0, 0.0, 2.0]}),
		json!({"kind": "sphere", "radius": 0.75, "offset": [-1.0, 0.0, 2.0]}),
		json!(null),
	] {
		let wire = request(collider);
		let decoded: EditorCommandRequest = serde_json::from_value(wire.clone()).unwrap();
		assert_eq!(serde_json::to_value(decoded).unwrap(), wire);
	}
	for collider in [
		json!({"kind": "box", "half_extents": [1.0, 1.0], "offset": [0, 0, 0]}),
		json!({"kind": "box", "half_extents": [1, 1, 1]}),
		json!({"kind": "box", "half_extents": [1, 1, 1], "offset": [0, 0, 0], "radius": 1.0}),
		json!({"kind": "sphere", "radius": 1.0, "offset": [0, 0, 0], "half_extents": [1, 1, 1]}),
		json!({"kind": "capsule", "radius": 1.0, "offset": [0, 0, 0]}),
	] {
		assert!(serde_json::from_value::<EditorCommandRequest>(request(collider)).is_err());
	}
}

#[test]
fn collider_overlay_contract_roundtrips_and_legacy_state_defaults_to_hidden() {
	use bridge::viewport::{ViewportActionDto, ViewportEditorStateDto};
	let mut state = ViewportEditorStateDto::default();
	assert!(!state.settings.colliders);
	assert!(state.collider_error.is_none());
	state.settings.colliders = true;
	state.collider_error = Some("Selected collider has nonfinite world geometry".into());
	let wire = serde_json::to_value(&state).unwrap();
	let decoded: ViewportEditorStateDto = serde_json::from_value(wire.clone()).unwrap();
	assert_eq!(decoded.settings, state.settings);
	assert_eq!(decoded.collider_error, state.collider_error);
	assert_eq!(serde_json::to_value(decoded).unwrap(), wire);
	let mut legacy = wire.clone();
	legacy["settings"]
		.as_object_mut()
		.unwrap()
		.remove("colliders");
	legacy.as_object_mut().unwrap().remove("collider_error");
	let decoded: ViewportEditorStateDto = serde_json::from_value(legacy).unwrap();
	assert!(!decoded.settings.colliders);
	assert!(decoded.collider_error.is_none());
	assert!(
		decoded.settings.grid,
		"The additive default must preserve other view settings"
	);
	for invalid in [json!("true"), json!(1), json!(null)] {
		let mut malformed = wire.clone();
		malformed["settings"]["colliders"] = invalid;
		assert!(serde_json::from_value::<ViewportEditorStateDto>(malformed).is_err());
	}
	for visible in [false, true] {
		let wire = json!({"type": "set_colliders", "visible": visible});
		let decoded: ViewportActionDto = serde_json::from_value(wire.clone()).unwrap();
		assert!(
			matches!(&decoded, ViewportActionDto::SetColliders { visible: actual } if *actual == visible)
		);
		decoded.validate().unwrap();
		assert_eq!(serde_json::to_value(decoded).unwrap(), wire);
	}
	for malformed in [
		json!({"type": "set_colliders"}),
		json!({"type": "set_colliders", "visible": "true"}),
		json!({"type": "set_colliders", "visible": null}),
		json!({"type": "set_colliders", "visible": true, "scene_id": "main"}),
	] {
		assert!(serde_json::from_value::<ViewportActionDto>(malformed).is_err());
	}
}

#[test]
fn collider_interaction_defaults_and_full_u32_settings_survive_command_transport() {
	use engine_core::scene_data::ColliderInteraction;
	let request = |collider| {
		json!({
			"expected_revision": 7,
			"command": {"type": "set_collider", "key": "crate", "collider": collider},
		})
	};
	for shape in [
		json!({"kind": "box", "half_extents": [0.5, 1.0, 2.0], "offset": [0.0, 0.0, 0.0]}),
		json!({"kind": "sphere", "radius": 0.75, "offset": [0.0, 0.0, 0.0]}),
	] {
		let legacy = request(shape.clone());
		let decoded: EditorCommandRequest = serde_json::from_value(legacy.clone()).unwrap();
		let EditorCommandDto::SetCollider {
			collider: Some(collider),
			..
		} = &decoded.command
		else {
			panic!("expected collider")
		};
		assert_eq!(
			collider.interaction(),
			ColliderInteraction {
				sensor: false,
				layers: 1,
				mask: u32::MAX
			}
		);
		assert_eq!(serde_json::to_value(decoded).unwrap(), legacy);
		for (settings, expected) in [
			(json!({}), ColliderInteraction::default()),
			(
				json!({"sensor": true}),
				ColliderInteraction {
					sensor: true,
					layers: 1,
					mask: u32::MAX,
				},
			),
			(
				json!({"layers": 2147483648_u32, "mask": 0}),
				ColliderInteraction {
					sensor: false,
					layers: 1 << 31,
					mask: 0,
				},
			),
			(
				json!({"layers": 0, "mask": 4294967295_u32}),
				ColliderInteraction {
					sensor: false,
					layers: 0,
					mask: u32::MAX,
				},
			),
			(
				json!({"sensor": true, "layers": 4294967295_u32, "mask": 4294967295_u32}),
				ColliderInteraction {
					sensor: true,
					layers: u32::MAX,
					mask: u32::MAX,
				},
			),
		] {
			let mut shape = shape.clone();
			shape["interaction"] = settings;
			let decoded: EditorCommandRequest = serde_json::from_value(request(shape)).unwrap();
			engine_core::editor::CommandFactory::validate(&decoded.command).unwrap();
			let EditorCommandDto::SetCollider {
				collider: Some(collider),
				..
			} = &decoded.command
			else {
				panic!("expected collider")
			};
			assert_eq!(collider.interaction(), expected);
			let encoded = serde_json::to_value(&decoded).unwrap();
			if expected == ColliderInteraction::default() {
				assert!(encoded["command"]["collider"].get("interaction").is_none());
			} else {
				assert_eq!(
					encoded["command"]["collider"]["interaction"],
					json!({"sensor": expected.sensor, "layers": expected.layers, "mask": expected.mask})
				);
			}
			let reloaded: EditorCommandRequest = serde_json::from_value(encoded).unwrap();
			assert_eq!(reloaded.command, decoded.command);
		}
	}
}

#[test]
fn collider_interaction_transport_rejects_unknown_fields_and_non_u32_filters() {
	for shape in [
		json!({"kind": "box", "half_extents": [0.5, 1.0, 2.0], "offset": [0, 0, 0]}),
		json!({"kind": "sphere", "radius": 0.75, "offset": [0, 0, 0]}),
	] {
		let mut invalid = vec![json!(null), json!([]), json!({"trigger": true})];
		for field in ["layers", "mask"] {
			for value in [
				json!(-1),
				json!(1.5),
				json!(4294967296_u64),
				json!("1"),
				json!(true),
				json!(null),
			] {
				let mut settings = json!({});
				settings[field] = value;
				invalid.push(settings);
			}
		}
		for value in [json!(0), json!(1), json!("true"), json!(null)] {
			invalid.push(json!({"sensor": value}));
		}
		for settings in invalid {
			let mut collider = shape.clone();
			collider["interaction"] = settings;
			let wire = json!({"expected_revision": 7, "command": {"type": "set_collider", "key": "crate", "collider": collider}});
			assert!(
				serde_json::from_value::<EditorCommandRequest>(wire.clone()).is_err(),
				"accepted malformed interaction: {wire}"
			);
		}
	}
}

#[test]
fn collision_contacts_accept_legacy_solid_data_and_preserve_sensor_classification() {
	use engine_core::collision::Contact;
	let legacy =
		json!({"a": "crate", "b": "region", "normal": [1.0, 0.0, 0.0], "penetration": 0.5});
	let decoded: Contact = serde_json::from_value(legacy.clone()).unwrap();
	assert!(!decoded.sensor);
	for sensor in [false, true] {
		let mut wire = legacy.clone();
		wire["sensor"] = json!(sensor);
		let decoded: Contact = serde_json::from_value(wire.clone()).unwrap();
		assert_eq!(decoded.sensor, sensor);
		assert_eq!(serde_json::to_value(decoded).unwrap(), wire);
	}
}
