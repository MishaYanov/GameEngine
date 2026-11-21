use bridge::editor::{EditorCommandDto, EditorCommandRequest};
use serde_json::json;

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
