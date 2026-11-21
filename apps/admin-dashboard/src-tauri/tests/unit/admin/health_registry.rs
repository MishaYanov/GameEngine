use super::*;

#[test]
fn registry_skips_unchanged_writes_and_retries_failed_changes() {
	let fixture = Fixture::new();
	let path = fixture.data().join("components.json");
	let mut registry = Registry::new();
	let mut entry = record("game-123-1", 1);
	entry.attributes = [
		("event".into(), "component_health".into()),
		("health".into(), "HEALTHY".into()),
	]
	.into();
	registry.accept(&entry);
	registry.persist(&path);
	let original = fs::read(&path).unwrap();
	// Block any new write without relying on filesystem timestamp resolution.
	let temporary = path.with_extension("tmp");
	fs::create_dir(&temporary).unwrap();
	registry.accept(&entry);
	registry.persist(&path);
	assert!(registry.persistence_error().is_none());
	assert_eq!(fs::read(&path).unwrap(), original);
	registry.stopped(&entry.session_id, 200, false);
	registry.persist(&path);
	assert!(registry.persistence_error().is_some());
	assert_eq!(fs::read(&path).unwrap(), original);
	fs::remove_dir(&temporary).unwrap();
	registry.persist(&path);
	assert!(registry.persistence_error().is_none());
	let restored = fs::read(&path).unwrap();
	assert_ne!(restored, original);
	fs::create_dir(&temporary).unwrap();
	registry.stopped(&entry.session_id, 200, false);
	registry.persist(&path);
	assert!(registry.persistence_error().is_none());
	let mut registry: Registry = serde_json::from_slice(&restored).unwrap();
	registry.persist(&path);
	assert!(registry.persistence_error().is_none());
	assert_eq!(fs::read(&path).unwrap(), restored);
}

#[test]
fn registry_checkpoint_recovery_preserves_independent_collector_failure() {
	let fixture = Fixture::new();
	let mut service = AdminService::new(fixture.0.clone(), fixture.logs(), fixture.data());
	let temporary = fixture.data().join("components.tmp");
	fs::create_dir(&temporary).unwrap();
	let mut entry = record("game-123-1", 1);
	entry.attributes = [("event".into(), "heartbeat".into())].into();
	service.registry.accept(&entry);
	service.tick();
	let registry_error = service.registry.persistence_error().unwrap().to_owned();
	assert_eq!(
		service.snapshot().collector.checkpoint_error,
		Some(registry_error)
	);
	service.collector.health.checkpoint_error = Some("Existing log checkpoint failure".into());
	fs::remove_dir(&temporary).unwrap();
	service.tick();
	assert!(service.registry.persistence_error().is_none());
	assert_eq!(
		service.snapshot().collector.checkpoint_error.as_deref(),
		Some("Existing log checkpoint failure")
	);
}
#[test]
fn health_uses_explicit_events_and_stale_heartbeats() {
	let mut registry = Registry::new();
	let mut entry = record("game-123-1", 1);
	entry.timestamp_ms = 100;
	entry.level = Level::Error;
	registry.accept(&entry);
	assert_eq!(registry.snapshot(100)[2].health, HealthState::Unknown);
	entry.attributes = [
		("event".into(), "component_health".into()),
		("health".into(), "HEALTHY".into()),
		("heartbeat_expected".into(), "true".into()),
	]
	.into();
	registry.accept(&entry);
	let game = |registry: &Registry, time| {
		registry
			.snapshot(time)
			.into_iter()
			.find(|c| c.id == "game")
			.unwrap()
	};
	assert_eq!(game(&registry, 100).health, HealthState::Healthy);
	assert!(game(&registry, 15101).stale);
	assert_eq!(game(&registry, 15101).health, HealthState::Degraded);
	entry.timestamp_ms = 16000;
	entry.attributes = [("event".into(), "heartbeat".into())].into();
	registry.accept(&entry);
	assert!(!game(&registry, 16001).stale);
	registry.stopped(&entry.session_id, 17000, true);
	assert_eq!(game(&registry, 17001).health, HealthState::Failed);
}
