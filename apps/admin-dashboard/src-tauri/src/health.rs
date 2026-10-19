use engine_logger::LogEntry;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum HealthState {
    Unknown,
    Healthy,
    Degraded,
    Failed,
    Stopped,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Component {
    pub id: String,
    pub label: String,
    pub health: HealthState,
    pub reported_health: HealthState,
    pub last_event_ms: Option<u64>,
    pub last_heartbeat_ms: Option<u64>,
    pub heartbeat_expected: bool,
    pub stale: bool,
    pub application: Option<String>,
    pub session_id: Option<String>,
    pub detail: String,
}
#[derive(Serialize, Deserialize)]
pub struct Registry {
    components: BTreeMap<String, Component>,
}
impl Registry {
    pub fn new() -> Self {
        let mut result = Self {
            components: BTreeMap::new(),
        };
        for (id, label) in [
            ("runtime", "Runtime"),
            ("renderer", "Renderer"),
            ("bridge", "Bridge"),
            ("compiler", "Compiler"),
            ("game", "Game application"),
            ("editor-host", "Client host"),
        ] {
            result.components.insert(
                id.into(),
                Component {
                    id: id.into(),
                    label: label.into(),
                    health: HealthState::Unknown,
                    reported_health: HealthState::Unknown,
                    last_event_ms: None,
                    last_heartbeat_ms: None,
                    heartbeat_expected: false,
                    stale: false,
                    application: None,
                    session_id: None,
                    detail: "Waiting for a lifecycle event".into(),
                },
            );
        }
        result
    }
    pub fn accept(&mut self, entry: &LogEntry) {
        let Some(event) = entry.attributes.get("event") else {
            return;
        };
        if event != "component_health" && event != "heartbeat" {
            return;
        }
        let Some(component) = self.components.get_mut(&entry.component) else {
            return;
        };
        if component
            .last_event_ms
            .is_some_and(|last| last > entry.timestamp_ms)
        {
            return;
        }
        if component
            .session_id
            .as_ref()
            .is_some_and(|id| id != &entry.session_id)
        {
            component.last_heartbeat_ms = None;
            component.heartbeat_expected = false;
            component.reported_health = HealthState::Unknown;
        }
        component.application = Some(entry.application.clone());
        component.session_id = Some(entry.session_id.clone());
        component.last_event_ms = Some(entry.timestamp_ms);
        component.detail = entry.message.clone();
        if event == "heartbeat" {
            component.last_heartbeat_ms = Some(entry.timestamp_ms);
            component.heartbeat_expected = true;
        } else if let Some(state) = entry.attributes.get("health").and_then(|v| {
            serde_json::from_value::<HealthState>(serde_json::Value::String(v.clone())).ok()
        }) {
            component.reported_health = state;
            component.heartbeat_expected = entry
                .attributes
                .get("heartbeat_expected")
                .is_some_and(|v| v == "true");
        }
    }
    pub fn snapshot(&self, now: u64) -> Vec<Component> {
        self.components
            .values()
            .cloned()
            .map(|mut component| {
                let last = component
                    .last_heartbeat_ms
                    .or(component.last_event_ms)
                    .unwrap_or(0);
                component.stale = component.heartbeat_expected
                    && component.reported_health != HealthState::Stopped
                    && now.saturating_sub(last) > 15_000;
                component.health =
                    if component.stale && component.reported_health == HealthState::Healthy {
                        HealthState::Degraded
                    } else {
                        component.reported_health
                    };
                if component.stale {
                    component.detail =
                        "Heartbeat is stale; application responsiveness is unknown".into();
                }
                component
            })
            .collect()
    }
    pub fn stopped(&mut self, session: &str, timestamp: u64, unexpected: bool) {
        for component in self
            .components
            .values_mut()
            .filter(|c| c.session_id.as_deref() == Some(session))
        {
            if component.last_event_ms.is_some_and(|last| last > timestamp) {
                continue;
            }
            component.reported_health = if unexpected {
                HealthState::Failed
            } else {
                HealthState::Stopped
            };
            component.heartbeat_expected = false;
            component.last_event_ms = Some(timestamp);
            component.detail = if unexpected {
                "Managed application exited unexpectedly"
            } else {
                "Managed application stopped"
            }
            .into();
        }
    }
}
impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn health_event(session: &str, timestamp: u64) -> LogEntry {
        serde_json::from_value(serde_json::json!({
            "schema_version": 1, "session_id": session, "sequence": 1,
            "timestamp_ms": timestamp, "application": "game", "component": "game",
            "process_id": 1, "thread": "test", "level": "INFO", "message": "ready",
            "file": null, "line": null, "module": null, "error": null, "stack_trace": null,
            "attributes": { "event": "component_health", "health": "HEALTHY", "heartbeat_expected": "true" },
            "correlation": { "operation_id": null, "project_id": null, "build_id": null, "test_run_id": null }
        })).unwrap()
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
}
