use super::*;
fn envelope(signal: Signal) -> Envelope {
	let endpoint = ComponentEndpoint {
		owner: RuntimeEntityHandle {
			world_id: "test".into(),
			key: EntityKey::new("switch").unwrap(),
			generation: 1,
		},
		component: ComponentId::new("logic").unwrap(),
	};
	Envelope {
		source: endpoint.clone(),
		target: endpoint,
		signal,
	}
}
#[test]
fn next_step_delivery_retains_emission_order_and_never_redrains_new_output() {
	let mut queue = EventQueue::default();
	let mut emitted = 0;
	let mut delivered = 0;
	for signal in [Signal::Reset, Signal::Activated, Signal::Collected] {
		queue.emit_next(9, &mut emitted, envelope(signal)).unwrap();
	}
	assert!(queue.take_due(9, &mut delivered).unwrap().is_empty());
	let frozen = queue.take_due(10, &mut delivered).unwrap();
	assert_eq!(
		frozen.iter().map(|e| e.signal).collect::<Vec<_>>(),
		[Signal::Reset, Signal::Activated, Signal::Collected]
	);
	queue
		.emit_next(10, &mut emitted, envelope(Signal::Released))
		.unwrap();
	assert!(queue.take_due(10, &mut delivered).unwrap().is_empty());
	assert_eq!(queue.take_due(11, &mut delivered).unwrap().len(), 1);
}
#[test]
fn exact_storage_and_producer_caps_reject_without_mutation() {
	let mut queue = EventQueue::default();
	for step in 0..2 {
		let mut emitted = 0;
		for _ in 0..MAX_EVENTS_PER_STEP {
			queue
				.emit_next(step, &mut emitted, envelope(Signal::Activated))
				.unwrap();
		}
		let before = queue.clone();
		assert!(
			queue
				.emit_next(step, &mut emitted, envelope(Signal::Reset))
				.is_err()
		);
		assert_eq!(queue, before);
		assert_eq!(emitted, MAX_EVENTS_PER_STEP);
	}
	assert_eq!(queue.len(), MAX_PENDING_EVENTS);
	let before = queue.clone();
	assert!(queue.emit_next(2, &mut 0, envelope(Signal::Reset)).is_err());
	assert_eq!(queue, before);
}
#[test]
fn delivery_budget_and_counter_overflow_do_not_consume_or_enqueue() {
	let mut queue = EventQueue::default();
	let mut emitted = 0;
	queue
		.emit_next(0, &mut emitted, envelope(Signal::Activated))
		.unwrap();
	let before = queue.clone();
	let mut delivered = MAX_DELIVERIES_PER_TICK;
	assert!(queue.take_due(1, &mut delivered).is_err());
	assert_eq!(queue, before);
	assert!(
		queue
			.emit_next(u64::MAX, &mut emitted, envelope(Signal::Reset))
			.is_err()
	);
	assert_eq!(queue, before);
	queue.next_sequence = MAX_SEQUENCE;
	let before = queue.clone();
	assert!(
		queue
			.emit_next(1, &mut emitted, envelope(Signal::Reset))
			.is_err()
	);
	assert_eq!(queue, before);
}
#[test]
fn retiring_either_endpoint_cancels_only_its_queued_messages() {
	let mut queue = EventQueue::default();
	let mut first = envelope(Signal::Activated);
	first.source.owner.key = EntityKey::new("sender").unwrap();
	let mut second = envelope(Signal::Reset);
	second.target.owner.key = EntityKey::new("receiver").unwrap();
	queue.emit_next(1, &mut 0, first).unwrap();
	queue.emit_next(1, &mut 0, second).unwrap();
	queue.remove_owners(&HashSet::from([EntityKey::new("sender").unwrap()]));
	assert_eq!(queue.len(), 1);
	queue.remove_owners(&HashSet::from([EntityKey::new("receiver").unwrap()]));
	assert_eq!(queue.len(), 0);
}
