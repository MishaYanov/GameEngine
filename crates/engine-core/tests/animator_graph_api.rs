mod support;

use engine_core::{
	animation::{
		AnimationCharacterIdentity, AnimationClipRef, AnimationSourceRef, AnimatorBinding,
		AnimatorCondition, AnimatorGraph, AnimatorNodePosition, AnimatorParameter,
		AnimatorParameterSource, AnimatorSettings, AnimatorState, AnimatorTransition,
	},
	asset_catalog::AssetId,
	command::{EditorCommandDto as Command, EditorCommandRequest},
	components::{ComponentData, ComponentId, ComponentRecord},
	editor::EditorSession,
};
use support::{entity, scene};

fn rig() -> AnimationSourceRef {
	AnimationSourceRef {
		asset_id: AssetId::generate(),
		expected_sha256: "a".repeat(64),
		scene_index: 0,
		mesh_node: 0,
		skin_index: 0,
		character: None,
	}
}

#[test]
fn character_joint_map_round_trips_inside_a_tagged_animator_binding() {
	let mut source = rig();
	source.character = Some(AnimationCharacterIdentity {
		clips: vec!["idle".into()],
		joints: [(3, "foot".into()), (5, "knee".into())].into(),
		root_node: Some(1),
	});
	let binding = AnimatorBinding::Imported { source };
	let json = serde_json::to_string(&binding).unwrap();
	assert_eq!(
		serde_json::from_str::<AnimatorBinding>(&json).unwrap(),
		binding
	);
	assert!(json.contains(r#""3":"foot""#));
	let duplicate = json.replace(r#""3":"foot""#, r#""3":"foot","3":"knee""#);
	assert!(
		serde_json::from_str::<AnimatorBinding>(&duplicate)
			.unwrap_err()
			.to_string()
			.contains("repeats a joint node")
	);
	let ambiguous = json.replace(r#""3":"foot""#, r#""03":"foot""#);
	assert!(
		serde_json::from_str::<AnimatorBinding>(&ambiguous)
			.unwrap_err()
			.to_string()
			.contains("canonical and bounded")
	);
}

fn graph(source: &AnimationSourceRef) -> AnimatorGraph {
	let state = |id: &str, name: &str, clip_index| AnimatorState {
		id: id.into(),
		name: name.into(),
		looping: true,
		clip: AnimationClipRef::Imported {
			source: source.clone(),
			clip_index,
		},
	};
	let edge = |id: &str, from: &str, to: &str, value| AnimatorTransition {
		id: id.into(),
		from: from.into(),
		to: to.into(),
		condition: AnimatorCondition::Bool {
			parameter: "moving".into(),
			value,
		},
		exit_after_ticks: None,
		blend_ticks: 6,
	};
	AnimatorGraph {
		schema_version: 1,
		entry: "idle".into(),
		states: vec![state("idle", "Idle", 0), state("walk", "Walk", 1)],
		parameters: vec![AnimatorParameter {
			id: "moving".into(),
			name: "Moving".into(),
			source: AnimatorParameterSource::Moving {
				minimum_mm_per_second: 150,
			},
		}],
		transitions: vec![
			edge("move", "idle", "walk", true),
			edge("rest", "walk", "idle", false),
		],
		layout: Default::default(),
	}
}

#[test]
fn animator_validates_stable_ids_ordered_edges_and_exact_rig_compatibility() {
	let source = rig();
	let mut graph = graph(&source);
	graph
		.validate_binding(&AnimatorBinding::Imported {
			source: source.clone(),
		})
		.unwrap();
	let bytes = graph.to_json().unwrap();
	assert_eq!(AnimatorGraph::from_json(&bytes).unwrap(), graph);
	assert!(
		graph
			.validate_binding(&AnimatorBinding::Imported { source: rig() })
			.is_err()
	);
	let digest = graph.runtime_digest().unwrap();
	graph.states[0].name = "Stand".into();
	graph
		.layout
		.insert("idle".into(), AnimatorNodePosition { x: 10, y: 20 });
	assert_eq!(graph.runtime_digest().unwrap(), digest);
	graph.transitions.reverse();
	assert_ne!(graph.runtime_digest().unwrap(), digest);
	graph.transitions[0].condition = AnimatorCondition::Trigger {
		parameter: "moving".into(),
	};
	assert!(graph.validate().unwrap_err().contains("rest"));
	graph.entry = "missing".into();
	assert!(graph.validate().unwrap_err().contains("entry"));
}

#[test]
fn two_objects_can_store_one_animator_and_legacy_playback_is_unaffected_until_admission() {
	let source = rig();
	let mut data = scene(vec![
		entity("first", [0.0; 3]),
		entity("second", [2.0, 0.0, 0.0]),
	]);
	for actor in &mut data.entities {
		actor.asset_id = Some(source.asset_id.clone());
	}
	let mut editor = EditorSession::new(data).unwrap();
	let command = |editor: &mut EditorSession, command| {
		editor.execute_command(EditorCommandRequest {
			expected_revision: editor.revision(),
			command,
		})
	};
	let settings = AnimatorSettings {
		root_motion_source_node: None,
		foot_ik: None,
		asset_id: AssetId::generate(),
		binding: AnimatorBinding::Imported { source },
	};
	for key in ["first", "second"] {
		command(
			&mut editor,
			Command::AddComponent {
				key: key.into(),
				component: ComponentRecord {
					id: ComponentId::generate(),
					schema_version: 1,
					enabled: true,
					data: ComponentData::Animator {
						settings: settings.clone(),
					},
				},
			},
		)
		.unwrap();
	}
	let document = editor.authored_document();
	let reopened = engine_core::SceneData::from_json(&document.to_json().unwrap()).unwrap();
	assert_eq!(reopened, *document);
	assert!(
		command(&mut editor, Command::StartPlay)
			.unwrap_err()
			.contains("Animator")
	);
	for _ in 0..2 {
		command(&mut editor, Command::Undo).unwrap();
	}
	command(&mut editor, Command::StartPlay).unwrap();
	assert!(editor.is_playing());
}
