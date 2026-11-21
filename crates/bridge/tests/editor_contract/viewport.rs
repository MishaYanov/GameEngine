use bridge::editor::ViewportBoundsDto;
use serde_json::json;

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
