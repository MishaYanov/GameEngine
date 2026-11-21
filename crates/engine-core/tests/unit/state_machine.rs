use super::*;
use crate::gameplay_events::Envelope;
use crate::{
	components::ComponentId, gameplay_events::ComponentEndpoint,
	runtime_lifecycle::RuntimeEntityHandle,
};
fn reference(key: &str) -> ComponentRef {
	ComponentRef {
		entity: EntityKey::new(key).unwrap(),
		component: ComponentId::new("logic").unwrap(),
	}
}
fn event(key: &str, signal: Signal) -> Envelope {
	let endpoint = ComponentEndpoint {
		owner: RuntimeEntityHandle {
			world_id: "test".into(),
			key: reference(key).entity,
			generation: 0,
		},
		component: reference(key).component,
	};
	Envelope {
		source: endpoint.clone(),
		target: endpoint,
		signal,
	}
}
fn state(id: &str, transitions: Vec<TransitionDef>) -> StateDef {
	StateDef {
		id: id.into(),
		on_enter: vec![],
		transitions,
	}
}
fn signal(source: Option<&str>, next: &str) -> TransitionDef {
	TransitionDef {
		when: MachineTrigger::Signal {
			signal: Signal::Activated,
			source: source.map(reference),
		},
		next: next.into(),
	}
}
fn definition(states: Vec<StateDef>) -> MachineDef {
	MachineDef {
		schema_version: 1,
		initial: states[0].id.clone(),
		inputs: vec![],
		states,
	}
}
#[test]
fn inbox_order_folds_all_signals_before_the_single_timer() {
	let def = definition(vec![
		state(
			"closed",
			vec![signal(Some("left"), "left"), signal(Some("right"), "right")],
		),
		state("left", vec![signal(Some("right"), "open")]),
		state("right", vec![signal(Some("left"), "open")]),
		state(
			"open",
			vec![TransitionDef {
				when: MachineTrigger::AfterTicks { ticks: 2 },
				next: "closed".into(),
			}],
		),
	]);
	let compiled = compile(&def).unwrap();
	for inbox in [
		vec![
			event("left", Signal::Activated),
			event("right", Signal::Activated),
		],
		vec![
			event("right", Signal::Activated),
			event("left", Signal::Activated),
		],
	] {
		let opened = evaluate(&compiled, &MachineInstance::default(), 10, &inbox).unwrap();
		assert_eq!(compiled.states[opened.next.state].id, "open");
		assert_eq!(opened.next.entered_step, 10);
		let waiting = evaluate(&compiled, &opened.next, 11, &[]).unwrap();
		assert_eq!(waiting.next, opened.next);
		let closed = evaluate(&compiled, &opened.next, 12, &[]).unwrap();
		assert_eq!(compiled.states[closed.next.state].id, "closed");
		assert_eq!(closed.next.entered_step, 12);
	}
}
#[test]
fn self_transition_reenters_and_signal_at_deadline_restarts_timer() {
	let mut def = definition(vec![
		state(
			"active",
			vec![
				signal(None, "active"),
				TransitionDef {
					when: MachineTrigger::AfterTicks { ticks: 2 },
					next: "expired".into(),
				},
			],
		),
		state("expired", vec![]),
	]);
	def.states[0].on_enter.push(MachineAction::SetVisible {
		entity: reference("visual").entity,
		visible: true,
	});
	let compiled = compile(&def).unwrap();
	let initial = evaluate(&compiled, &MachineInstance::default(), 1, &[]).unwrap();
	assert_eq!(initial.effects.len(), 1);
	let reentered = evaluate(
		&compiled,
		&initial.next,
		3,
		&[event("source", Signal::Activated)],
	)
	.unwrap();
	assert_eq!(reentered.next.state, 0);
	assert_eq!(reentered.next.entered_step, 3);
	assert_eq!(reentered.effects.len(), 1);
	assert_eq!(
		evaluate(&compiled, &reentered.next, 5, &[])
			.unwrap()
			.next
			.state,
		1
	);
}
#[test]
fn transition_budget_is_exact_and_evaluation_does_not_mutate_input() {
	let compiled = compile(&definition(vec![state("loop", vec![signal(None, "loop")])])).unwrap();
	let current = MachineInstance::default();
	let events = vec![event("source", Signal::Activated); 32];
	assert!(evaluate(&compiled, &current, 1, &events).is_ok());
	let mut excess = events;
	excess.push(event("source", Signal::Activated));
	assert!(
		evaluate(&compiled, &current, 1, &excess)
			.unwrap_err()
			.contains("transition budget")
	);
	assert_eq!(current, MachineInstance::default());
}
#[test]
fn malformed_tables_fail_before_compilation_and_unknown_fields_are_rejected() {
	let valid = definition(vec![state("ready", vec![])]);
	let mut invalid = valid.clone();
	invalid.states[0].transitions = vec![signal(None, "ready"), signal(Some("source"), "ready")];
	assert!(invalid.validate().unwrap_err().contains("overlap"));
	invalid = valid.clone();
	invalid.states[0].transitions = vec![signal(None, "missing")];
	assert!(invalid.validate().is_err());
	invalid = valid.clone();
	invalid.states[0].transitions = vec![TransitionDef {
		when: MachineTrigger::AfterTicks { ticks: 0 },
		next: "ready".into(),
	}];
	assert!(invalid.validate().is_err());
	invalid = valid.clone();
	invalid.states.push(invalid.states[0].clone());
	assert!(invalid.validate().is_err());
	invalid = valid.clone();
	invalid.states[0].id = "bad id".into();
	assert!(invalid.validate().is_err());
	let mut json = serde_json::to_value(valid).unwrap();
	json["runtime_state"] = serde_json::json!("ready");
	assert!(serde_json::from_value::<MachineDef>(json).is_err());
}
#[test]
fn remap_changes_typed_references_without_rewriting_state_or_input_ids() {
	let mut def = definition(vec![state("same", vec![signal(Some("same"), "same")])]);
	def.inputs.push(SensorInput {
		id: "same".into(),
		sensor: reference("same").entity,
		actor: reference("actor").entity,
		signal: Signal::Activated,
	});
	def.states[0].on_enter = vec![
		MachineAction::Emit {
			target: reference("same"),
			signal: Signal::Reset,
		},
		MachineAction::SetVisible {
			entity: reference("same").entity,
			visible: false,
		},
	];
	def.remap(&HashMap::from([(
		reference("same").entity,
		reference("copy").entity,
	)]));
	assert_eq!(def.initial, "same");
	assert_eq!(def.states[0].id, "same");
	assert_eq!(def.inputs[0].id, "same");
	assert_eq!(def.states[0].transitions[0].next, "same");
	assert!(
		def.entity_references()
			.iter()
			.all(|key| key.as_str() != "same")
	);
}
#[test]
fn byte_budget_counts_serialized_nested_definitions() {
	let target = reference(&"x".repeat(256));
	let states = (0..32)
		.map(|index| StateDef {
			id: format!("state-{index}"),
			on_enter: vec![
				MachineAction::Emit {
					target: target.clone(),
					signal: Signal::Activated
				};
				8
			],
			transitions: vec![],
		})
		.collect();
	let def = definition(states);
	assert!(serde_json::to_vec(&def).unwrap().len() > MAX_DEFINITION_BYTES);
	assert!(def.validate().unwrap_err().contains("64 KiB"));
}
