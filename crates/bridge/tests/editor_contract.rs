use bridge::editor::{EditorCommandDto, EditorCommandRequest, ViewportBoundsDto};
use serde_json::json;

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
