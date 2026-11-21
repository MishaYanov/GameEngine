mod support;

use engine_core::{
	Engine, SceneData,
	command::{EditorCommandDto as Command, EditorCommandRequest},
	game_camera::{FollowCameraSettings, OrbitCameraSettings, required_probe_radius},
	project::ProjectData,
	scene_data::parse_json,
};
use serde_json::{Value, json};
use support::{entity, key, scene};

fn orbit() -> OrbitCameraSettings {
	OrbitCameraSettings {
		target: key("actor"),
		pivot_offset: [0.0, 0.7, 0.0],
		initial_yaw: 0.25,
		initial_pitch: -0.4,
		initial_distance: 4.5,
		min_distance: 0.5,
		max_distance: 12.0,
		min_pitch: -1.3,
		max_pitch: 0.9,
		follow_seconds: 0.1,
		recovery_seconds: 0.2,
		teleport_distance: 8.0,
		probe_radius: 0.25,
		obstruction_mask: 0x8000_0001,
	}
}

fn follow() -> FollowCameraSettings {
	FollowCameraSettings {
		target: key("actor"),
		offset: [0.0, 2.0, 5.0],
		look_offset: [0.0; 3],
		smoothing_seconds: 0.3,
	}
}

fn orbit_scene() -> SceneData {
	let mut document = scene(vec![
		entity("parent", [1.0, 0.0, 0.0]),
		entity("actor", [0.0; 3]).with_parent(Some(key("parent"))),
		entity("decoration", [4.0, 0.0, 0.0]),
	]);
	document.camera.orbit = Some(orbit());
	document
}

fn disk(document: &SceneData) -> Value {
	serde_json::from_str(&document.to_json().unwrap()).unwrap()
}

#[test]
fn orbit_survives_disk_and_editor_projection_without_changing_fallback_pose() {
	let document = orbit_scene();
	let saved = disk(&document);
	assert_eq!(saved["schema_version"], 5);
	assert!(saved["entities"][0].get("components").is_some());
	assert!(saved["camera"].get("follow").is_none());
	assert_eq!(saved["camera"]["orbit"]["obstruction_mask"], 0x8000_0001u32);
	assert_eq!(parse_json(&saved.to_string()).unwrap().data, document);
	let projection = serde_json::to_value(&document).unwrap();
	assert!(projection["entities"][0].get("model_path").is_some());
	assert_eq!(
		serde_json::from_value::<SceneData>(projection).unwrap(),
		document
	);
}

#[test]
fn schema_four_fixed_and_follow_migrate_from_canonical_disk_and_flat_editor_projection() {
	for follow in [None, Some(follow())] {
		let mut document = orbit_scene();
		document.camera.orbit = None;
		document.camera.follow = follow;
		for mut historical in [disk(&document), serde_json::to_value(&document).unwrap()] {
			historical["schema_version"] = json!(4);
			assert!(historical["camera"].get("orbit").is_none());
			let migrated = parse_json(&historical.to_string()).unwrap();
			assert_eq!(migrated.source_schema_version, 4);
			assert_eq!(migrated.data, document);
			assert_eq!(disk(&migrated.data)["schema_version"], 5);
			assert_eq!(
				serde_json::from_value::<SceneData>(historical).unwrap(),
				document
			);
		}
	}
}

#[test]
fn orbit_requires_schema_five_even_when_null_or_under_a_flat_projection() {
	let document = orbit_scene();
	for mut value in [disk(&document), serde_json::to_value(&document).unwrap()] {
		let orbit = value["camera"]["orbit"].clone();
		for version in 1..=4 {
			value["schema_version"] = json!(version);
			for field in [&orbit, &Value::Null] {
				value["camera"]["orbit"] = field.clone();
				assert!(
					parse_json(&value.to_string()).is_err(),
					"schema {version} accepted orbit"
				);
				assert!(serde_json::from_value::<SceneData>(value.clone()).is_err());
			}
		}
	}
}

#[test]
fn rejects_malformed_references_unknown_settings_and_conflicting_camera_modes() {
	let settings = serde_json::to_value(orbit()).unwrap();
	for target in [
		String::new(),
		"not a key".into(),
		"bad\u{0001}key".into(),
		"é".repeat(129),
	] {
		let mut value = settings.clone();
		value["target"] = json!(target);
		let decoded: OrbitCameraSettings = serde_json::from_value(value).unwrap();
		assert!(decoded.validate().is_err());
	}

	let mut invalid = disk(&orbit_scene());
	invalid["camera"]["orbit"]["target"] = json!("missing");
	assert!(
		SceneData::from_json(&invalid.to_string())
			.unwrap_err()
			.contains("target")
	);
	invalid["camera"]["orbit"]["target"] = json!("actor");
	invalid["camera"]["orbit"]["current_yaw"] = json!(1.0);
	assert!(SceneData::from_json(&invalid.to_string()).is_err());

	let mut document = orbit_scene();
	document.camera.follow = Some(follow());
	assert!(document.to_json().is_err());
}

#[test]
fn orbit_enforces_finite_tuning_bounds_and_ordered_ranges() {
	let base = orbit();
	for (field, invalid) in [
		("pivot_offset", json!([0.0, 100.1, 0.0])),
		("initial_yaw", json!(-3.2)),
		("initial_pitch", json!(1.1)),
		("min_pitch", json!(base.max_pitch)),
		("max_pitch", json!(1.5)),
		("min_distance", json!(0.24)),
		("max_distance", json!(101.0)),
		("initial_distance", json!(0.49)),
		("initial_distance", json!(13.0)),
		("follow_seconds", json!(-0.1)),
		("recovery_seconds", json!(5.1)),
		("teleport_distance", json!(0.9)),
		("probe_radius", json!(2.01)),
		("obstruction_mask", json!(0)),
	] {
		let mut value = serde_json::to_value(&base).unwrap();
		value[field] = invalid;
		let candidate: OrbitCameraSettings = serde_json::from_value(value).unwrap();
		assert!(candidate.validate().is_err(), "invalid {field} accepted");
	}
	for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
		let settings = OrbitCameraSettings {
			initial_distance: invalid,
			..base.clone()
		};
		assert!(settings.validate().is_err());
		let settings = OrbitCameraSettings {
			pivot_offset: [invalid; 3],
			..base.clone()
		};
		assert!(settings.validate().is_err());
	}

	let boundary = OrbitCameraSettings {
		pivot_offset: [-100.0, 100.0, 0.0],
		min_distance: 0.25,
		max_distance: 100.0,
		initial_distance: 0.25,
		min_pitch: -85.0_f32.to_radians(),
		max_pitch: 85.0_f32.to_radians(),
		initial_pitch: 85.0_f32.to_radians(),
		initial_yaw: std::f32::consts::PI,
		follow_seconds: 0.0,
		recovery_seconds: 5.0,
		teleport_distance: 1000.0,
		probe_radius: 2.0,
		obstruction_mask: u32::MAX,
		..base
	};
	boundary.validate().unwrap();
}

#[test]
fn envelope_covers_near_corner_with_margin_and_checks_the_exact_f32_boundary() {
	let (near, fov) = (0.1_f32, 60.0_f32.to_radians());
	let half_height = f64::from(near) * (f64::from(fov) / 2.0).tan();
	let corner = (f64::from(near).powi(2) + half_height.powi(2) + (3.0 * half_height).powi(2))
		.sqrt()
		+ 0.001;
	let required = required_probe_radius(near, fov).unwrap();
	let below = f32::from_bits(required.to_bits() - 1);
	assert!(f64::from(required) >= corner);
	assert!(f64::from(below) < corner);
	let mut document = orbit_scene();
	document.camera.orbit.as_mut().unwrap().probe_radius = required;
	document.validate().unwrap();
	document.camera.orbit.as_mut().unwrap().probe_radius = below;
	assert!(document.validate().is_err());
	document.camera.orbit.as_mut().unwrap().probe_radius = required;
	document.camera.near_plane = 1.0;
	assert!(document.validate().is_err());
	for (n, f) in [
		(0.0, fov),
		(0.1, f32::NAN),
		(0.1, std::f32::consts::PI),
		(10.0, fov),
	] {
		assert!(required_probe_radius(n, f).is_err());
	}
}

fn command(engine: &mut Engine, command: Command) {
	engine
		.execute(EditorCommandRequest {
			expected_revision: engine.session().revision(),
			command,
		})
		.unwrap();
}

#[test]
fn duplicating_the_target_keeps_the_camera_bound_to_the_original() {
	let document = orbit_scene();
	let project = ProjectData::for_scene("Orbit", "scenes/main.json", &document).unwrap();
	let mut engine = Engine::new(document.clone(), project).unwrap();
	command(
		&mut engine,
		Command::Select {
			keys: vec!["actor".into()],
		},
	);
	command(
		&mut engine,
		Command::DuplicateSelection {
			offset: [1.0, 0.0, 0.0],
		},
	);
	let after = engine.snapshot().unwrap();
	assert_eq!(after.document.entities.len(), document.entities.len() + 1);
	assert_eq!(after.selection.len(), 1);
	assert_ne!(after.selection[0], "actor");
	assert_eq!(after.document.camera, document.camera);
}

#[test]
fn deleting_target_subtree_clears_orbit_and_one_undo_restores_the_whole_edit() {
	let document = orbit_scene();
	let project = ProjectData::for_scene("Orbit", "scenes/main.json", &document).unwrap();
	let mut engine = Engine::new(document.clone(), project).unwrap();

	command(
		&mut engine,
		Command::Select {
			keys: vec!["decoration".into()],
		},
	);
	command(&mut engine, Command::DeleteSelection);
	assert_eq!(engine.session().authored_document().camera, document.camera);
	command(&mut engine, Command::Undo);
	command(
		&mut engine,
		Command::Select {
			keys: vec!["parent".into()],
		},
	);
	command(&mut engine, Command::DeleteSelection);
	let after = engine.session().authored_document();
	assert!(
		after
			.entities
			.iter()
			.all(|entity| entity.key != key("actor"))
	);
	assert!(after.camera.orbit.is_none());
	command(&mut engine, Command::Undo);
	assert_eq!(engine.session().authored_document().as_ref(), &document);
	command(&mut engine, Command::Redo);
	assert_eq!(engine.session().authored_document(), after);
}

#[test]
fn invalid_set_camera_preserves_selection_history_revision_and_journal_cursor() {
	let document = orbit_scene();
	let project = ProjectData::for_scene("Orbit", "scenes/main.json", &document).unwrap();
	let mut engine = Engine::new(document.clone(), project).unwrap();
	command(
		&mut engine,
		Command::Select {
			keys: vec!["actor".into()],
		},
	);
	let mut changed = document.camera.clone();
	changed.orbit.as_mut().unwrap().initial_distance = 7.0;
	command(
		&mut engine,
		Command::SetCamera {
			camera: changed.clone(),
		},
	);
	command(&mut engine, Command::Undo);
	let before = engine.snapshot().unwrap();
	assert!(before.can_redo);
	let cursor = engine.change_cursor();

	let mut missing = changed.clone();
	missing.orbit.as_mut().unwrap().target = key("removed");
	let mut undersized = changed.clone();
	undersized.near_plane = 1.0;
	for camera in [missing, undersized] {
		let rejected = engine.execute(EditorCommandRequest {
			expected_revision: before.revision,
			command: Command::SetCamera { camera },
		});
		assert!(rejected.is_err());
		let after = engine.snapshot().unwrap();
		assert_eq!(after.document, before.document);
		assert_eq!(after.selection, before.selection);
		assert_eq!(after.revision, before.revision);
		assert_eq!(after.scene_revision, before.scene_revision);
		assert_eq!(after.dirty, before.dirty);
		assert_eq!(
			(after.can_undo, after.can_redo),
			(before.can_undo, before.can_redo)
		);
		assert_eq!(engine.change_cursor(), cursor);
	}
	command(&mut engine, Command::Redo);
	assert_eq!(engine.session().authored_document().camera, changed);
}
