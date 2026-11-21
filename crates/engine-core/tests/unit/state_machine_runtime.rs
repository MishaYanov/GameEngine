use super::*;
#[test]
fn duplicate_enter_pairs_emit_once_per_route_in_stable_machine_order() {
	let document = crate::SceneData::from_json(include_str!(
		"../../../../apps/game/assets/scenes/event-lab.json"
	))
	.unwrap();
	let world = RuntimeWorld::new(Arc::new(document)).unwrap();
	let mut lifecycle = RuntimeLifecycle::new(&world);
	let event = |sensor: &str, phase| TriggerEvent {
		scene_id: crate::scene_catalog::SceneId::new("event-lab").unwrap(),
		step: 1,
		phase,
		sensor: crate::EntityKey::new(sensor).unwrap(),
		other: crate::EntityKey::new("event-player").unwrap(),
	};
	let left = event("a-left", TriggerPhase::Enter);
	let right = event("a-right", TriggerPhase::Enter);
	let exit = event("b-left", TriggerPhase::Exit);
	let mut emitted = 0;
	route_sensors(
		&world,
		&mut lifecycle,
		1,
		&[right.clone(), left.clone(), right, left, exit],
		&mut emitted,
	)
	.unwrap();
	assert_eq!(emitted, 2);
	let inbox = lifecycle.events.take_due(2, &mut 0).unwrap();
	assert_eq!(
		inbox
			.iter()
			.map(|e| e.target.owner.key.as_str())
			.collect::<Vec<_>>(),
		["a-left", "a-right"]
	);
	assert!(inbox.iter().all(|e| e.source == e.target));
}
