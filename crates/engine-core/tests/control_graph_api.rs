mod support;

use engine_core::{
	Engine, EngineBuilder, Key,
	animation::{
		AnimationClipRef, AnimatorGraph, AnimatorParameter, AnimatorParameterSource, AnimatorState,
	},
	asset_catalog::AssetId,
	command::{EditorCommandDto as Command, EditorCommandRequest},
	components::{ComponentData, InputActionsRef},
	input::{
		ActionBinding, ControlButton, ControlGraph, ControlGraphFrame, ControlGraphOutput,
		ControlLook, ControlNode, ControlNodeData as Node, ControlNodePosition, FaceButton,
		InputActionKind as Kind, InputActionMap, InputProfile, InputState, NamedInputAction,
		PhysicalGamepadSample, ResolvedInputActions, control_action_types,
	},
	project::ProjectData,
	scene_catalog::SceneId,
	scene_data::ColliderData,
	simulation_session::{ActionWriter, GameModule, GameModuleFactory, InputFrame},
	world::RuntimeWorld,
};
use std::{
	collections::{BTreeSet, HashSet},
	sync::{Arc, Mutex},
};
use support::{entity, scene};

fn node(id: &str, data: Node) -> ControlNode {
	ControlNode {
		id: id.into(),
		data,
	}
}

fn action(id: &str, value: Kind) -> ControlNode {
	node(
		id,
		Node::Action {
			action: id.into(),
			value,
		},
	)
}

fn controls() -> ControlGraph {
	ControlGraph {
		schema_version: 1,
		nodes: vec![
			node(
				"strike",
				Node::Attack {
					input: "aimed_attack".into(),
				},
			),
			node(
				"aimed_attack",
				Node::Gate {
					condition: "aim_held".into(),
					input: "attack".into(),
				},
			),
			node(
				"aiming",
				Node::AnimatorBool {
					parameter: "aiming".into(),
					input: "aim_held".into(),
				},
			),
			node(
				"aim_held",
				Node::ButtonHeld {
					input: "aim".into(),
				},
			),
			node(
				"motor",
				Node::Move {
					input: "move".into(),
				},
			),
			node(
				"lift",
				Node::Jump {
					input: "jump".into(),
				},
			),
			node(
				"view",
				Node::Look {
					input: "look".into(),
				},
			),
			action("attack", Kind::Button),
			action("aim", Kind::Button),
			action("jump", Kind::Button),
			action("move", Kind::Vector2),
			action("look", Kind::Delta2),
		],
		layout: Default::default(),
	}
}

fn actions() -> InputActionMap {
	let mut map = InputActionMap::starter();
	map.actions.push(NamedInputAction {
		id: "aim".into(),
		label: "Aim".into(),
		value: Kind::Button,
		bindings: vec![
			ActionBinding::Key { key: Key::G },
			ActionBinding::GamepadButton {
				button: FaceButton::Y,
			},
		],
	});
	map.validate().unwrap();
	map
}

fn animator() -> AnimatorGraph {
	AnimatorGraph {
		schema_version: 1,
		entry: "idle".into(),
		states: vec![AnimatorState {
			id: "idle".into(),
			name: "Idle".into(),
			looping: true,
			clip: AnimationClipRef::Authored {
				asset_id: AssetId::generate(),
				expected_sha256: "1".repeat(64),
			},
		}],
		parameters: vec![AnimatorParameter {
			id: "aiming".into(),
			name: "Aiming".into(),
			source: AnimatorParameterSource::Boolean { default: false },
		}],
		transitions: vec![],
		layout: Default::default(),
	}
}

#[test]
fn stable_order_matches_authored_edges_and_ignores_node_array_and_layout_order() {
	let animator = animator();
	let types = control_action_types(Some(&actions()), &InputProfile::default());
	let mut graph = controls();
	let order: Vec<String> = graph
		.compile(&types, Some(&animator))
		.unwrap()
		.order()
		.map(str::to_owned)
		.collect();
	graph.nodes.reverse();
	graph
		.layout
		.insert("aim".into(), ControlNodePosition { x: 100, y: -80 });
	let bytes = graph.to_json().unwrap();
	assert_eq!(ControlGraph::from_json(&bytes).unwrap(), graph);
	let compiled = graph.compile(&types, Some(&animator)).unwrap();
	assert!(compiled.order().eq(order.iter().map(String::as_str)));
	assert!(
		order.iter().position(|id| id == "aim_held") < order.iter().position(|id| id == "strike")
	);
}

#[test]
fn keyboard_and_standard_pad_have_equivalent_gated_intents_and_per_step_edges() {
	let map = actions();
	let types = control_action_types(Some(&map), &InputProfile::default());
	let graph = controls().compile(&types, Some(&animator())).unwrap();
	let mut keys = InputState::default();
	for key in [Key::W, Key::F, Key::G, Key::Space] {
		keys.press(key);
	}
	let keyboard = map.resolve(&keys, Default::default(), [0.0; 2]);
	let pad = map.resolve(
		&InputState::default(),
		PhysicalGamepadSample {
			left_y: i16::MAX,
			face_buttons: [true, false, true, true],
			..Default::default()
		},
		[0.0; 2],
	);
	let resolve = |actions: &ResolvedInputActions, pressed: &[&str], released: &[&str]| {
		graph.evaluate(&ControlGraphFrame {
			actions,
			pressed: &pressed.iter().map(|id| (*id).into()).collect(),
			released: &released.iter().map(|id| (*id).into()).collect(),
		})
	};
	let edge = resolve(&keyboard, &["jump", "attack"], &[]);
	let pad_edge = resolve(&pad, &["jump", "attack"], &[]);
	assert_eq!(edge.intent.jump, pad_edge.intent.jump);
	assert_eq!(edge.intent.attack, pad_edge.intent.attack);
	assert_eq!(edge.presentation, pad_edge.presentation);
	assert!(
		edge.intent
			.movement
			.into_iter()
			.zip(pad_edge.intent.movement)
			.all(|(keyboard, controller)| (keyboard - controller).abs() < 0.0001)
	);
	assert_ne!(edge.intent.movement, [0.0; 2]);
	assert!(edge.intent.jump.pressed && edge.intent.attack.pressed && edge.presentation["aiming"]);
	let held = resolve(&keyboard, &[], &[]);
	assert!(held.intent.attack.held);
	assert!(!held.intent.attack.pressed);
	keys.release(Key::F);
	let release = resolve(
		&map.resolve(&keys, Default::default(), [0.0; 2]),
		&[],
		&["attack"],
	);
	assert_eq!(
		release.intent.attack,
		ControlButton {
			released: true,
			..Default::default()
		}
	);
	keys.release(Key::G);
	keys.press(Key::F);
	let blocked = resolve(
		&map.resolve(&keys, Default::default(), [0.0; 2]),
		&["attack"],
		&[],
	);
	assert_eq!(blocked.intent.attack, ControlButton::default());
	assert!(!blocked.presentation["aiming"]);
	keys.suppress_held();
	let focus_lost = resolve(&map.resolve(&keys, Default::default(), [0.0; 2]), &[], &[]);
	assert_eq!(focus_lost.intent, ControlGraphOutput::default().intent);
}

#[test]
fn invalid_ports_outputs_cycles_and_presentation_sources_are_rejected_with_node_diagnostics() {
	let mut graph = controls();
	graph.nodes.insert(
		0,
		node(
			"other_move",
			Node::Move {
				input: "move".into(),
			},
		),
	);
	assert!(graph.validate().unwrap_err().contains("duplicates output"));
	graph.nodes[0] = node(
		"other_out",
		Node::AnimatorBool {
			parameter: "aiming".into(),
			input: "aim_held".into(),
		},
	);
	assert!(graph.validate().unwrap_err().contains("duplicates output"));
	graph.nodes[0] = node(
		"wrong_jump",
		Node::Jump {
			input: "aim_held".into(),
		},
	);
	assert!(graph.validate().unwrap_err().contains("wrong_jump"));
	graph.nodes[0] = node(
		"read_output",
		Node::Not {
			input: "aiming".into(),
		},
	);
	assert!(
		graph
			.validate()
			.unwrap_err()
			.contains("cannot read from output")
	);
	graph.nodes[0] = node(
		"missing_node",
		Node::Not {
			input: "absent".into(),
		},
	);
	assert!(graph.validate().unwrap_err().contains("missing node"));
	graph.nodes[0] = node(
		"cycle",
		Node::Not {
			input: "cycle".into(),
		},
	);
	assert!(graph.validate().unwrap_err().contains("cycle"));
	graph.nodes.remove(0);
	let mut animator = animator();
	let mut types = control_action_types(Some(&actions()), &InputProfile::default());
	types.remove("attack");
	assert!(
		graph
			.compile(&types, Some(&animator))
			.unwrap_err()
			.contains("attack")
	);
	types.insert("attack".into(), Kind::Vector2);
	assert!(
		graph
			.compile(&types, Some(&animator))
			.unwrap_err()
			.contains("attack")
	);
	types.insert("attack".into(), Kind::Button);
	assert!(graph.compile(&types, None).unwrap_err().contains("aiming"));
	for source in [
		AnimatorParameterSource::Grounded,
		AnimatorParameterSource::Jumped,
		AnimatorParameterSource::ActionAccepted {
			action: "attack".into(),
		},
	] {
		animator.parameters[0].source = source;
		assert!(
			graph
				.compile(&types, Some(&animator))
				.unwrap_err()
				.contains("aiming")
		);
	}
	assert!(
		ControlGraph::from_json(&vec![b' '; 128 * 1024 + 1])
			.unwrap_err()
			.contains("128 KiB")
	);
	assert!(
		ControlGraph::from_json(br#"{"schema_version":1,"nodes":[],"unexpected":true}"#).is_err()
	);
}

#[test]
fn external_frames_have_bounded_motion_and_keep_a_tap_in_one_step() {
	let map = actions();
	let types = control_action_types(Some(&map), &InputProfile::default());
	let graph = controls().compile(&types, Some(&animator())).unwrap();
	let mut actions = ResolvedInputActions::default();
	actions.vectors.insert("move".into(), [f32::MAX, f32::MAX]);
	actions.deltas.insert("look".into(), [f32::NAN, 22.0]);
	actions.look_rates.insert("look".into(), [1.0, 0.0]);
	let edges = BTreeSet::from(["jump".into()]);
	let output = graph.evaluate(&ControlGraphFrame {
		actions: &actions,
		pressed: &edges,
		released: &edges,
	});
	assert!(
		output
			.intent
			.movement
			.into_iter()
			.all(|axis| (axis - std::f32::consts::FRAC_1_SQRT_2).abs() < f32::EPSILON)
	);
	assert_eq!(
		output.intent.look,
		ControlLook {
			delta: [0.0; 2],
			rate: [1.0, 0.0]
		}
	);
	assert_eq!(
		output.intent.jump,
		ControlButton {
			held: false,
			pressed: true,
			released: true
		}
	);
}

#[derive(Clone, Default)]
struct RecordAttack(Arc<Mutex<Vec<(bool, bool)>>>);

impl GameModuleFactory for RecordAttack {
	fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
		Ok(Box::new(self.clone()))
	}
}

impl GameModule for RecordAttack {
	fn needs_tick(&self, _: &InputFrame) -> bool {
		true
	}

	fn plan(
		&self,
		_: &RuntimeWorld,
		input: &InputFrame,
		_: f32,
		_: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		self.0
			.lock()
			.unwrap()
			.push((input.action_held("attack"), input.action_pressed("attack")));
		Ok(())
	}
}

#[test]
fn play_routes_motor_and_gameplay_through_one_gate_per_step_and_requires_declared_bindings() {
	const STEP: f64 = 1.0 / 60.0;
	let mut floor = entity("floor", [0.0, -0.5, 0.0]);
	floor.collider = Some(ColliderData::Box {
		half_extents: [20.0, 0.5, 20.0],
		offset: [0.0; 3],
		interaction: Default::default(),
	});
	let document = scene(vec![floor]);
	let project = ProjectData::for_scene("Controls", "scenes/main.json", &document).unwrap();
	let recorder = RecordAttack::default();
	let mut graph = controls();
	graph.nodes.retain(|node| node.id != "aiming");
	for (motor, source) in [("motor", "move"), ("lift", "jump")] {
		let gate = format!("{source}_gate");
		graph.nodes.push(node(
			&gate,
			Node::Gate {
				condition: "aim_held".into(),
				input: source.into(),
			},
		));
		let target = graph
			.nodes
			.iter_mut()
			.find(|node| node.id == motor)
			.unwrap();
		target.data = if motor == "motor" {
			Node::Move { input: gate }
		} else {
			Node::Jump { input: gate }
		};
	}
	let mut engine = EngineBuilder::new(document, project)
		.module_factory(Arc::new(recorder.clone()))
		.input_actions_resolver(Arc::new(move |_| Ok(Some(actions()))))
		.control_graph_resolver(Arc::new(move |project, scene, map| {
			if scene.local_player().is_none() {
				return Ok(None);
			}
			let profile = project.input_profile.clone().unwrap_or_default();
			graph
				.compile(&control_action_types(map, &profile), None)
				.map(Some)
		}))
		.build()
		.unwrap();
	let exec = |engine: &mut Engine, command| {
		engine.execute(EditorCommandRequest {
			expected_revision: engine.session().revision(),
			command,
		})
	};
	exec(&mut engine, Command::CreatePlayer).unwrap();
	let object = engine
		.session()
		.authored_document()
		.entities
		.last()
		.unwrap()
		.clone();
	let actor = object.key.clone();
	let mut component = object
		.component_records()
		.into_iter()
		.find(|component| matches!(component.data, ComponentData::PlayerController { .. }))
		.unwrap();
	let ComponentData::PlayerController { settings } = &mut component.data else {
		unreachable!()
	};
	settings.input = InputActionsRef::Asset {
		asset_id: AssetId::generate(),
	};
	let graph_id = AssetId::generate();
	settings.control_graph = Some(graph_id.clone());
	exec(
		&mut engine,
		Command::ReplaceComponent {
			key: actor.as_str().into(),
			component,
		},
	)
	.unwrap();
	exec(
		&mut engine,
		Command::SetLocalPlayer {
			key: Some(actor.clone()),
		},
	)
	.unwrap();
	let mut unresolved = EngineBuilder::new(
		engine.session().authored_document().as_ref().clone(),
		engine.session().project().clone(),
	)
	.input_actions_resolver(Arc::new(move |_| Ok(Some(actions()))))
	.build()
	.unwrap();
	let invalid = exec(&mut unresolved, Command::StartPlay).unwrap_err();
	assert!(invalid.message.contains("Controls Graph") && invalid.message.contains(actor.as_str()));
	assert!(!unresolved.session().is_playing());
	exec(&mut engine, Command::StartPlay).unwrap();
	let sample = |engine: &mut Engine, keys: &[&str]| {
		engine
			.set_action_controls_from_codes(
				&keys.iter().map(|key| (*key).into()).collect::<HashSet<_>>(),
				Default::default(),
				Default::default(),
			)
			.unwrap();
	};
	let position = |engine: &Engine| {
		engine
			.session()
			.render_document()
			.entities
			.iter()
			.find(|entity| entity.key == actor)
			.unwrap()
			.transform
			.position
	};
	for _ in 0..4 {
		engine.tick(STEP).unwrap();
	}
	let origin = position(&engine);
	sample(&mut engine, &["KeyF", "KeyW", "Space"]);
	engine.tick(STEP).unwrap();
	assert_eq!(position(&engine), origin);
	assert_eq!(recorder.0.lock().unwrap().last(), Some(&(false, false)));
	let blocked = engine.runtime_inspection(0, 16).unwrap();
	let blocked_step = blocked.control_graph.unwrap();
	assert_eq!(blocked_step.actor.key, actor);
	assert_eq!(blocked_step.asset_id, graph_id);
	assert_eq!(blocked_step.tick, blocked.tick);
	assert!(blocked_step.active_node_ids.contains(&"attack".into()));
	assert!(!blocked_step.active_node_ids.contains(&"strike".into()));
	sample(&mut engine, &[]);
	engine.tick(STEP).unwrap();
	sample(&mut engine, &["KeyF", "KeyG", "KeyW", "Space"]);
	let outcome = engine.tick(STEP * 3.0).unwrap();
	assert_eq!(outcome.steps, 3);
	assert!(outcome.accepted_action_presses.is_empty());
	let moved = position(&engine);
	assert_ne!(moved[2], origin[2]);
	assert!(moved[1] > origin[1]);
	assert_eq!(
		recorder
			.0
			.lock()
			.unwrap()
			.iter()
			.filter(|&&(_, pressed)| pressed)
			.count(),
		1
	);
	assert_eq!(recorder.0.lock().unwrap().last(), Some(&(true, false)));
	let held = engine
		.runtime_inspection(0, 16)
		.unwrap()
		.control_graph
		.unwrap();
	assert_eq!(
		held.output.intent.attack,
		ControlButton {
			held: true,
			..Default::default()
		}
	);
	assert!(held.active_node_ids.contains(&"strike".into()));
	sample(&mut engine, &["KeyG"]);
	engine.tick(STEP).unwrap();
	let released = engine
		.runtime_inspection(0, 16)
		.unwrap()
		.control_graph
		.unwrap();
	assert!(released.output.intent.attack.released);
	for name in ["move", "jump", "attack"] {
		assert!(released.changed_outputs.contains(&name.into()));
	}
	exec(&mut engine, Command::SetPaused { paused: true }).unwrap();
	assert!(
		engine
			.runtime_inspection(0, 16)
			.unwrap()
			.control_graph
			.is_none()
	);
	exec(&mut engine, Command::SetPaused { paused: false }).unwrap();
	engine.tick(STEP).unwrap();
	assert_eq!(recorder.0.lock().unwrap().last(), Some(&(false, false)));
	exec(&mut engine, Command::StopPlay).unwrap();
	assert!(engine.runtime_inspection(0, 16).is_err());
	exec(&mut engine, Command::StartPlay).unwrap();
	let restarted = engine.runtime_inspection(0, 16).unwrap();
	assert_ne!(blocked.run_id, restarted.run_id);
	assert!(restarted.control_graph.is_none());
}
