mod support;
use engine_core::{
	BackendErrorCode, Engine, EngineBuilder,
	asset_registry::AssetRegistryLimits,
	command::{EditorCommandDto as Command, EditorCommandRequest},
	events::BackendEventKind,
	project::{BehaviorSpec, ProjectData},
	simulation_session::{
		ActionWriter, GameModule, GameModuleFactory, InputFrame, SimulationAction,
	},
	world::RuntimeWorld,
};
use std::sync::Arc;
use support::*;
fn fixture() -> Engine {
	let data = scene(vec![
		entity("actor", [0.0; 3]),
		entity("other", [1.0, 0.0, 0.0]),
	]);
	let mut project = ProjectData::for_scene("Project", "scenes/main.json", &data).unwrap();
	project.add_behavior(BehaviorSpec::ControlledMove {
		entity: key("actor"),
		speed: 2.0,
		space: Default::default(),
	});
	EngineBuilder::new(data, project).build().unwrap()
}
fn run(engine: &mut Engine, command: Command) -> engine_core::CommandReceipt {
	engine
		.execute(EditorCommandRequest {
			expected_revision: engine.session().revision(),
			command,
		})
		.unwrap()
}

#[test]
fn builder_validates_authored_scene_project_and_resource_limits_without_graphics() {
	let data = scene(vec![entity("actor", [0.0; 3])]);
	let mut invalid = ProjectData::for_scene("Project", "scenes/main.json", &data).unwrap();
	invalid.add_behavior(BehaviorSpec::Spin {
		entity: key("missing"),
		angular_velocity: [0.0, 1.0, 0.0],
	});
	let error = EngineBuilder::new(data.clone(), invalid)
		.build()
		.err()
		.unwrap();
	assert_eq!(error.code, BackendErrorCode::InvalidCommand);
	assert!(error.current_revision.is_none());
	let project = ProjectData::for_scene("Project", "scenes/main.json", &data).unwrap();
	let error = EngineBuilder::new(data, project)
		.asset_limits(AssetRegistryLimits {
			max_assets: 0,
			..Default::default()
		})
		.build()
		.err()
		.unwrap();
	assert_eq!(error.code, BackendErrorCode::InvalidCommand);
}

#[test]
fn receipt_and_change_feed_distinguish_noop_selection_document_and_project_changes() {
	let mut engine = fixture();
	let noop = run(
		&mut engine,
		Command::Rename {
			key: "actor".into(),
			name: "actor".into(),
		},
	);
	assert!(!noop.state_changed);
	assert_eq!(noop.change_cursor, 0);
	assert!(noop.changes.is_empty());
	let selection = run(
		&mut engine,
		Command::Select {
			keys: vec!["other".into()],
		},
	);
	assert!(selection.state_changed);
	assert!(selection.changes.is_empty());
	assert!(!selection.project_changed);
	let page = engine.changes_since(0).unwrap();
	assert_eq!(page.events.len(), 1);
	assert!(matches!(
		page.events[0].event,
		BackendEventKind::StateChanged
	));
	let changed = run(
		&mut engine,
		Command::Rename {
			key: "actor".into(),
			name: "Renamed".into(),
		},
	);
	assert_eq!(changed.changes.changed_entities, vec![key("actor")]);
	assert!(!changed.project_changed);
	let mut project = engine.session().project().clone();
	project.name = "Configured".into();
	let configured = run(&mut engine, Command::SetProject { project });
	assert!(configured.project_changed);
	assert!(configured.changes.is_empty());
	let page = engine.changes_since(changed.change_cursor).unwrap();
	assert_eq!(page.events.len(), 1);
	assert!(matches!(
		page.events[0].event,
		BackendEventKind::ProjectChanged
	));
}

#[test]
fn typed_conflict_invalid_command_and_domain_rejection_preserve_state_and_event_cursor() {
	let mut engine = fixture();
	let before = serde_json::to_value(engine.snapshot().unwrap()).unwrap();
	for (revision, command, expected) in [
		(1, Command::DeleteSelection, BackendErrorCode::Conflict),
		(
			0,
			Command::Rename {
				key: "actor".into(),
				name: "\n".into(),
			},
			BackendErrorCode::InvalidCommand,
		),
		(
			0,
			Command::Rename {
				key: "missing".into(),
				name: "Valid".into(),
			},
			BackendErrorCode::CommandRejected,
		),
	] {
		let error = engine
			.execute(EditorCommandRequest {
				expected_revision: revision,
				command,
			})
			.unwrap_err();
		assert_eq!(error.code, expected);
		assert_eq!(error.current_revision, Some(0));
		assert_eq!(
			serde_json::to_value(engine.snapshot().unwrap()).unwrap(),
			before
		);
		assert_eq!(engine.change_cursor(), 0);
	}
	assert_eq!(
		engine.changes_since(1).unwrap_err().code,
		BackendErrorCode::InvalidCursor
	);
	assert_eq!(
		engine.tick(f64::NAN).unwrap_err().code,
		BackendErrorCode::InvalidCommand
	);
	assert_eq!(engine.change_cursor(), 0);
}

#[test]
fn play_ticks_publish_render_changes_without_invalidating_expected_command_revision() {
	let mut engine = fixture();
	run(
		&mut engine,
		Command::Select {
			keys: vec!["other".into()],
		},
	);
	let start = run(&mut engine, Command::StartPlay);
	assert!(start.changes.full_resync);
	run(
		&mut engine,
		Command::SetEntityInput {
			entity: "actor".into(),
			movement: [1.0, 0.0, 0.0],
		},
	);
	let revision = engine.session().revision();
	let cursor = engine.change_cursor();
	let scene_revision = engine.session().scene_revision();
	let authored = engine.session().authored_document();
	let tick = engine.tick(0.1).unwrap();
	assert_eq!(tick.steps, 6);
	assert_eq!(engine.session().revision(), revision);
	assert!(engine.session().scene_revision() > scene_revision);
	assert_eq!(engine.session().authored_document(), authored);
	close(
		engine.session().render_document().entities[0]
			.transform
			.position[0],
		0.2,
	);
	let events = engine.changes_since(cursor).unwrap();
	assert_eq!(events.events.len(), 1);
	assert!(matches!(
		events.events[0].event,
		BackendEventKind::SimulationAdvanced { tick: 6, .. }
	));
	assert_eq!(events.events[0].revision, revision);
	let stop = engine
		.execute(EditorCommandRequest {
			expected_revision: revision,
			command: Command::StopPlay,
		})
		.unwrap();
	assert!(stop.changes.full_resync);
	assert_eq!(engine.session().render_document(), authored);
}

#[test]
fn project_and_scene_boundaries_preserve_independent_dirty_baselines_and_event_monotonicity() {
	let mut engine = fixture();
	run(
		&mut engine,
		Command::Rename {
			key: "actor".into(),
			name: "Edited".into(),
		},
	);
	let mut project = engine.session().project().clone();
	project.name = "Edited project".into();
	run(&mut engine, Command::SetProject { project });
	let cursor = engine.change_cursor();
	engine.mark_scene_saved().unwrap();
	assert!(engine.session().is_dirty());
	assert!(engine.change_cursor() > cursor);
	engine.mark_project_saved().unwrap();
	assert!(!engine.session().is_dirty());
	let cursor = engine.change_cursor();
	assert!(!engine.mark_saved().unwrap().state_changed);
	assert_eq!(engine.change_cursor(), cursor);
	let revision = engine.session().revision();
	engine.supersede_revision(revision + 10).unwrap();
	assert!(engine.session().revision() > revision + 10);
	assert!(engine.change_cursor() > cursor);
}

#[test]
fn coordinator_journal_is_bounded_and_slow_consumers_receive_resync_marker() {
	let mut engine = fixture();
	for n in 0..260 {
		run(
			&mut engine,
			Command::Rename {
				key: "actor".into(),
				name: format!("Name {n}"),
			},
		);
	}
	let page = engine.changes_since(0).unwrap();
	assert!(page.resync_required);
	assert!(page.events.is_empty());
	assert!(engine.retained_event_bytes() <= engine_core::events::ChangeJournal::MAX_BYTES);
	assert!(engine.changes_since(page.cursor).unwrap().events.is_empty());
}

struct BadFactory;
impl GameModuleFactory for BadFactory {
	fn create(
		&self,
		_p: &ProjectData,
		_: &engine_core::scene_catalog::SceneId,
	) -> Result<Box<dyn GameModule>, String> {
		Ok(Box::new(BadModule))
	}
}
struct BadModule;
impl GameModule for BadModule {
	fn needs_tick(&self, _i: &InputFrame) -> bool {
		true
	}
	fn plan(
		&self,
		_w: &RuntimeWorld,
		_i: &InputFrame,
		_s: f32,
		out: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		out.push(SimulationAction::Move {
			entity: key("missing"),
			world_delta: [1.0; 3],
		})
	}
}
#[test]
fn simulation_rejection_rolls_back_and_publishes_pause_state_as_a_typed_error() {
	let data = scene(vec![entity("actor", [0.0; 3])]);
	let project = ProjectData::for_scene("Project", "scenes/main.json", &data).unwrap();
	let mut engine = EngineBuilder::new(data.clone(), project)
		.module_factory(Arc::new(BadFactory))
		.build()
		.unwrap();
	run(&mut engine, Command::StartPlay);
	let cursor = engine.change_cursor();
	let revision = engine.session().revision();
	let error = engine.tick(0.1).unwrap_err();
	assert_eq!(error.code, BackendErrorCode::SimulationRejected);
	assert_eq!(error.current_revision, Some(revision));
	assert!(engine.session().is_paused());
	assert_eq!(engine.session().render_document().as_ref(), &data);
	let page = engine.changes_since(cursor).unwrap();
	assert_eq!(page.events.len(), 1);
	assert!(matches!(
		page.events[0].event,
		BackendEventKind::StateChanged
	));
}

#[test]
fn typed_scene_growth_respects_aggregate_authored_budget_before_committing_history_or_events() {
	let mut engine = fixture();
	let before = serde_json::to_value(engine.snapshot().unwrap()).unwrap();
	let oversized = (0..20_000)
		.map(|i| entity(&format!("large-{i}"), [0.0; 3]))
		.collect::<Vec<_>>();
	// Small individual DTOs exceed the aggregate budget; no giant file or GPU allocation is needed.
	let data = scene(oversized.clone());
	assert!(data.entities.len() < engine_core::scene_data::MAX_SCENE_ENTITIES);
	assert!(data.validate().unwrap_err().contains("32 MiB"));
	assert!(data.to_json().is_err());
	let error = engine
		.replace_entities(oversized, vec![], "Oversized import")
		.unwrap_err();
	assert_eq!(error.code, BackendErrorCode::CommandRejected);
	assert!(error.message.contains("32 MiB"));
	assert_eq!(
		serde_json::to_value(engine.snapshot().unwrap()).unwrap(),
		before
	);
	assert_eq!(engine.change_cursor(), 0);
}

#[test]
fn paired_entity_and_asset_adoption_commits_both_or_preserves_registry_history_and_events() {
	use engine_core::asset_registry::{AssetMetadata, AssetRef};
	let mut engine = fixture();
	let reference = AssetRef::new("models/new.glb", Some(0)).unwrap();
	let metadata = AssetMetadata {
		estimated_bytes: 128,
		..Default::default()
	};
	let before = serde_json::to_value(engine.snapshot().unwrap()).unwrap();
	let cursor = engine.change_cursor();
	let invalid = vec![entity("duplicate", [0.0; 3]), entity("duplicate", [1.0; 3])];
	assert!(
		engine
			.replace_entities_with_assets(
				invalid,
				vec![],
				"Rejected import",
				vec![(reference.clone(), metadata.clone())]
			)
			.is_err()
	);
	assert!(engine.assets().is_empty());
	assert_eq!(engine.assets().metadata_bytes(), 0);
	assert_eq!(
		serde_json::to_value(engine.snapshot().unwrap()).unwrap(),
		before
	);
	assert_eq!(engine.change_cursor(), cursor);
	let document = engine.session().authored_document();
	let entities = vec![entity("new", [2.0, 0.0, 0.0])];
	let invalid_metadata = AssetMetadata {
		estimated_bytes: u64::MAX,
		..Default::default()
	};
	assert!(
		engine
			.replace_entities_with_assets(
				entities.clone(),
				vec![key("new")],
				"Rejected metadata",
				vec![(reference.clone(), invalid_metadata)]
			)
			.is_err()
	);
	assert_eq!(
		serde_json::to_value(engine.snapshot().unwrap()).unwrap(),
		before
	);
	assert!(engine.assets().is_empty());
	assert_eq!(engine.change_cursor(), cursor);
	engine
		.replace_entities_with_assets(
			entities,
			vec![key("new")],
			"Import",
			vec![(reference.clone(), metadata.clone())],
		)
		.unwrap();
	let handle = engine.assets().find(&reference).unwrap();
	assert_eq!(engine.assets().get(handle).unwrap().metadata, metadata);
	assert_eq!(engine.session().authored_document().entities.len(), 3);
	run(&mut engine, Command::Undo);
	assert_eq!(engine.session().authored_document(), document);
	run(&mut engine, Command::Redo);
	assert_eq!(engine.session().authored_document().entities.len(), 3);
	engine.clear_assets();
	assert!(engine.assets().get(handle).is_none());
}
