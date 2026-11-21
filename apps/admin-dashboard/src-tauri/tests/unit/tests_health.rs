use super::*;

fn health_event(session: &str, timestamp: u64) -> LogEntry {
	serde_json::from_value(serde_json::json!({
		"schema_version": 1, "session_id": session, "sequence": 1,
		"timestamp_ms": timestamp, "application": "game", "component": "game",
		"process_id": 1, "thread": "test", "level": "INFO", "message": "ready",
		"file": null, "line": null, "module": null, "error": null, "stack_trace": null,
		"attributes": { "event": "component_health", "health": "HEALTHY", "heartbeat_expected": "true" },
		"correlation": { "operation_id": null, "project_id": null, "build_id": null, "test_run_id": null }
	}))
	.unwrap()
}

#[test]
fn heartbeat_boundary_and_clock_skew_do_not_prematurely_degrade_health() {
	let mut registry = Registry::new();
	registry.accept(&health_event("session-a", 1000));
	let game = |now| {
		registry
			.snapshot(now)
			.into_iter()
			.find(|c| c.id == "game")
			.unwrap()
	};
	assert_eq!(game(999).health, HealthState::Healthy);
	assert_eq!(game(16_000).health, HealthState::Healthy);
	assert_eq!(game(16_001).health, HealthState::Degraded);
}

#[test]
fn old_events_and_old_session_exit_cannot_overwrite_a_restarted_game() {
	let mut registry = Registry::new();
	registry.accept(&health_event("session-a", 1000));
	registry.accept(&health_event("session-b", 2000));
	let mut old = health_event("session-a", 1500);
	old.attributes.insert("health".into(), "FAILED".into());
	registry.accept(&old);
	registry.stopped("session-a", 3000, true);
	let game = registry
		.snapshot(3000)
		.into_iter()
		.find(|c| c.id == "game")
		.unwrap();
	assert_eq!(game.session_id.as_deref(), Some("session-b"));
	assert_eq!(game.health, HealthState::Healthy);
}
