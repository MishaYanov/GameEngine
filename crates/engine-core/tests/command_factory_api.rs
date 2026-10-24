use engine_core::{
	command::{
		EditorCommandDto as Command, EntitySpecDto, TransformDto, TransformEditDto, TransformSpace,
	},
	editor::{CommandFactory, EditorSession},
};
mod support;
use support::*;

#[test]
fn factory_builder_requires_an_action_preserves_expected_revision_and_shares_execution_validation()
{
	assert!(CommandFactory::builder(10).build().is_err());
	let request = CommandFactory::builder(0)
		.command(Command::Rename {
			key: "a".into(),
			name: "Changed".into(),
		})
		.build()
		.unwrap();
	assert_eq!(request.expected_revision, 0);
	let mut session = EditorSession::new(scene(vec![entity("a", [0.0; 3])])).unwrap();
	session.execute(request).unwrap();
	assert_eq!(session.authored_document().entities[0].name, "Changed");
	let before = session.authored_document();
	let revision = session.revision();
	let stale = CommandFactory::build(
		0,
		Command::Rename {
			key: "a".into(),
			name: "Stale".into(),
		},
	)
		.unwrap();
	assert!(session.execute(stale).is_err());
	assert_eq!(session.authored_document(), before);
	assert_eq!(session.revision(), revision);
}

#[test]
fn factory_rejects_malformed_keys_names_paths_and_duplicate_transform_targets_before_execution() {
	for command in [
		Command::Rename {
			key: "bad key".into(),
			name: "Valid".into(),
		},
		Command::Rename {
			key: "a".into(),
			name: "line\nbreak".into(),
		},
		Command::Spawn {
			entity: EntitySpecDto {
				name: "New".into(),
				model_path: "../escape.glb".into(),
				transform: TransformDto::default(),
				parent: None,
				mesh_index: None,
			},
		},
		Command::SetTransforms {
			edits: vec![
				TransformEditDto {
					key: "a".into(),
					transform: TransformDto::default()
				};
				2
			],
			space: TransformSpace::Local,
			snap: false,
		},
		Command::Select {
			keys: vec!["a".into(); 4097],
		},
	] {
		assert!(CommandFactory::build(0, command).is_err());
	}
}

#[test]
fn factory_bounds_pick_extent_and_coordinates_and_rejects_nonfinite_transform_intent() {
	for (pixel, extent) in [
		([0.0, 0.0], [0, 600]),
		([800.0, 300.0], [800, 600]),
		([f64::NAN, 0.0], [800, 600]),
		([1.0, 1.0], [16385, 600]),
	] {
		assert!(
			CommandFactory::build(
				0,
				Command::PickSelection {
					pixel,
					extent,
					additive: false
				}
			)
				.is_err()
		);
	}
	for command in [
		Command::TranslateSelection {
			delta: [f32::NAN; 3],
			snap: false,
		},
		Command::RotateSelection {
			radians: [f32::INFINITY; 3],
			space: TransformSpace::World,
			snap: false,
		},
		Command::ScaleSelection {
			factor: [f32::NEG_INFINITY; 3],
			snap: false,
		},
		Command::SetPlayInput {
			movement: [f32::NAN; 3],
		},
	] {
		assert!(CommandFactory::build(0, command).is_err());
	}
	assert!(
		CommandFactory::build(
			0,
			Command::PickSelection {
				pixel: [799.0, 599.0],
				extent: [800, 600],
				additive: false
			}
		)
			.is_ok()
	);
}
