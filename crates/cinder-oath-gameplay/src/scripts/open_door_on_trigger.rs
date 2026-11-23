//! Authoring descriptor for the trigger-driven door.

use engine_core::components::{
	ScriptDescriptor, ScriptField, ScriptReferenceField, ScriptReferenceKind,
};

pub(crate) fn descriptor() -> ScriptDescriptor {
	ScriptDescriptor {
		id: "project.open_door_on_trigger".into(),
		label: "Open Door on Trigger".into(),
		config_version: 1,
		fields: vec![
			ScriptField::Number {
				key: "open_angle_degrees".into(),
				label: "Open angle (degrees)".into(),
				default: 90.0,
				min: 0.0,
				max: 180.0,
			},
			ScriptField::Number {
				key: "speed_degrees_per_second".into(),
				label: "Speed (degrees per second)".into(),
				default: 180.0,
				min: 1.0,
				max: 720.0,
			},
		],
		references: vec![ScriptReferenceField {
			key: "sensor".into(),
			label: "Trigger sensor".into(),
			requires: ScriptReferenceKind::TriggerCollider,
		}],
		receive_local_actions: false,
		write_owner_transform: true,
	}
}
