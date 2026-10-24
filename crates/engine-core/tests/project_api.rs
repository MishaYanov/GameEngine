mod support;
use engine_core::project::{
	BehaviorSpace, BehaviorSpec, MAX_BEHAVIORS, MAX_PROJECT_BYTES, ProjectData,
};
use serde_json::json;
use std::fs;
use support::*;

#[test]
fn a_new_authored_project_has_no_implicit_demo_behavior_and_roundtrips_without_scene_bindings() {
	let scene = scene(vec![entity("actor", [0.0; 3])]);
	let project = ProjectData::for_scene("My project", "scenes/main.json", &scene).unwrap();
	assert!(project.behaviors.is_empty());
	assert_eq!(project.fixed_hz, 60);
	assert_eq!(project.max_catchup_steps, 8);
	assert_eq!(
		ProjectData::from_json(&project.to_json().unwrap()).unwrap(),
		project
	);
	let defaults = ProjectData::from_json(
		r#"{"schema_version":1,"name":"Project","startup_scene":"scenes/main.json"}"#,
	)
	.unwrap();
	assert!(defaults.behaviors.is_empty());
	assert_eq!(defaults.fixed_hz, 60);
}

#[test]
fn behaviors_reference_explicit_persistent_targets_independent_of_scene_order_or_labels() {
	let mut scene = scene(vec![entity("a", [0.0; 3]), entity("b", [2.0, 0.0, 0.0])]);
	let mut project = ProjectData::for_scene("Project", "scenes/main.json", &scene).unwrap();
	project.behaviors = vec![
		BehaviorSpec::ControlledMove {
			entity: key("b"),
			speed: 2.5,
			space: BehaviorSpace::World,
		},
		BehaviorSpec::Spin {
			entity: key("a"),
			angular_velocity: [0.0, 1.0, 0.0],
		},
	];
	project.validate_scene(&scene).unwrap();
	scene.entities.reverse();
	scene.entities[0].name = "Same label".into();
	scene.entities[1].name = "Same label".into();
	project.validate_scene(&scene).unwrap();
	assert_eq!(
		ProjectData::from_json(&project.to_json().unwrap()).unwrap(),
		project
	);
	scene.entities.retain(|e| e.key != key("b"));
	assert!(project.validate_scene(&scene).is_err());
}

#[test]
fn project_manifest_rejects_unknown_fields_invalid_paths_and_unbounded_timing() {
	let original = ProjectData::for_scene("Project", "scenes/main.json", &scene(vec![])).unwrap();
	for path in [
		"../main.json",
		"C:/main.json",
		"scenes\\main.json",
		"scenes/main.glb",
		"",
	] {
		let mut invalid = original.clone();
		invalid.startup_scene = path.into();
		assert!(invalid.validate().is_err(), "accepted {path:?}");
	}
	for (hz, budget) in [(0, 8), (1001, 8), (60, 0), (60, 9)] {
		let mut invalid = original.clone();
		invalid.fixed_hz = hz;
		invalid.max_catchup_steps = budget;
		assert!(invalid.validate().is_err());
	}
	for name in ["", " \t", "line\nbreak"] {
		let mut invalid = original.clone();
		invalid.name = name.into();
		assert!(invalid.validate().is_err());
	}
	let mut unknown = serde_json::to_value(original).unwrap();
	unknown["unexpected"] = json!(true);
	assert!(ProjectData::from_json(&unknown.to_string()).is_err());
}

#[test]
fn malformed_behavior_configuration_is_rejected_without_requiring_gpu_or_demo_roles() {
	let mut project = ProjectData::for_scene(
		"Project",
		"scenes/main.json",
		&scene(vec![entity("actor", [0.0; 3])]),
	)
	.unwrap();
	for behavior in [
		BehaviorSpec::ControlledMove {
			entity: key("actor"),
			speed: -1.0,
			space: BehaviorSpace::World,
		},
		BehaviorSpec::ControlledMove {
			entity: key("actor"),
			speed: f32::NAN,
			space: BehaviorSpace::Local,
		},
		BehaviorSpec::Spin {
			entity: key("actor"),
			angular_velocity: [0.0, f32::INFINITY, 0.0],
		},
		BehaviorSpec::Spin {
			entity: key("actor"),
			angular_velocity: [1_000_001.0, 0.0, 0.0],
		},
	] {
		project.behaviors = vec![behavior];
		assert!(project.validate().is_err());
	}
	project.behaviors = vec![
		BehaviorSpec::Spin {
			entity: key("actor"),
			angular_velocity: [0.0; 3]
		};
		MAX_BEHAVIORS + 1
	];
	assert!(project.validate().is_err());
	let invalid = json!({"schema_version":1,"name":"Project","startup_scene":"scenes/main.json","behaviors":[{"kind":"controlled_move","entity":"actor","speed":1,"unexpected":true}]});
	assert!(ProjectData::from_json(&invalid.to_string()).is_err());
}

#[test]
fn project_loader_bounds_reads_and_preserves_the_source_file() {
	let directory = Directory::new("project");
	let path = directory.0.join("project.json");
	let project = ProjectData::for_scene("Project", "scenes/main.json", &scene(vec![])).unwrap();
	let source = project.to_json().unwrap();
	fs::write(&path, &source).unwrap();
	assert_eq!(ProjectData::load(&path).unwrap(), project);
	assert_eq!(fs::read_to_string(&path).unwrap(), source);
	let file = fs::File::create(&path).unwrap();
	file.set_len(MAX_PROJECT_BYTES as u64 + 1).unwrap();
	drop(file);
	assert!(ProjectData::load(&path).is_err());
	assert!(ProjectData::load(&directory.0).is_err());
	assert!(ProjectData::load(directory.0.join("missing.json")).is_err());
}
