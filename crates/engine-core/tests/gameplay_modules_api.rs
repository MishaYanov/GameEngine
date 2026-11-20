mod support;

use engine_core::{
	BackendError, Engine, EngineBuilder,
	command::{EditorCapabilitiesDto, EditorCommandDto as Command, EditorCommandRequest},
	project::ProjectData,
	scene_catalog::SceneId,
	simulation_session::{GameModule, GameModuleFactory, resolve_default_gameplay},
};
use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};
use support::scene;

struct IncompleteModule(Arc<AtomicUsize>);
impl GameModuleFactory for IncompleteModule {
	fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
		self.0.fetch_add(1, Ordering::Relaxed);
		Err("Assign the required roles before Play".into())
	}
}

fn execute(engine: &mut Engine, command: Command) -> Result<(), BackendError> {
	engine
		.execute(EditorCommandRequest {
			expected_revision: engine.session().revision(),
			command,
		})
		.map(|_| ())
}

fn choose(engine: &mut Engine, id: Option<&str>) -> Result<(), BackendError> {
	let mut project = engine.session().project().clone();
	project.gameplay_module = id.map(String::from);
	execute(engine, Command::SetProject { project })
}

#[test]
fn module_selection_is_undoable_and_defers_scene_preparation_until_play() {
	let document = scene(vec![]);
	let project = ProjectData::for_scene("Empty project", "scenes/main.json", &document).unwrap();
	let resolved = Arc::new(AtomicUsize::new(0));
	let prepared = Arc::new(AtomicUsize::new(0));
	let factory: Arc<dyn GameModuleFactory> = Arc::new(IncompleteModule(Arc::clone(&prepared)));
	let calls = Arc::clone(&resolved);
	let mut engine = EngineBuilder::new(document.clone(), project)
		.module_resolver(Arc::new(move |project| {
			calls.fetch_add(1, Ordering::Relaxed);
			if project.gameplay_module.as_deref() == Some("known_module") {
				Ok(Arc::clone(&factory))
			} else {
				resolve_default_gameplay(project)
			}
		}))
		.build()
		.unwrap();

	choose(&mut engine, Some("known_module")).unwrap();
	let selected = engine.snapshot().unwrap();
	assert!(selected.project_dirty);
	assert_eq!(selected.document, document);
	assert_eq!(resolved.load(Ordering::Relaxed), 1);
	assert_eq!(prepared.load(Ordering::Relaxed), 0);
	choose(&mut engine, Some("known_module")).unwrap();
	assert_eq!(engine.session().revision(), selected.revision);
	assert_eq!(resolved.load(Ordering::Relaxed), 1);
	execute(&mut engine, Command::Undo).unwrap();
	assert!(engine.session().project().gameplay_module.is_none());
	assert!(!engine.snapshot().unwrap().project_dirty);
	execute(&mut engine, Command::Redo).unwrap();
	assert_eq!(engine.session().project(), &selected.project);
	assert_eq!(resolved.load(Ordering::Relaxed), 1);
	assert_eq!(prepared.load(Ordering::Relaxed), 0);

	let before = serde_json::to_value(engine.snapshot().unwrap()).unwrap();
	let error = execute(&mut engine, Command::StartPlay).unwrap_err();
	assert!(error.message.contains("required roles"));
	assert_eq!(prepared.load(Ordering::Relaxed), 1);
	assert_eq!(
		serde_json::to_value(engine.snapshot().unwrap()).unwrap(),
		before
	);

	execute(&mut engine, Command::Undo).unwrap();
	let before = serde_json::to_value(engine.snapshot().unwrap()).unwrap();
	let cursor = engine.change_cursor();
	assert!(choose(&mut engine, Some("missing_module")).is_err());
	assert_eq!(
		serde_json::to_value(engine.snapshot().unwrap()).unwrap(),
		before
	);
	assert_eq!(engine.change_cursor(), cursor);
	execute(&mut engine, Command::Redo).unwrap();
	assert_eq!(engine.session().project(), &selected.project);

	choose(&mut engine, None).unwrap();
	execute(&mut engine, Command::StartPlay).unwrap();
	let before = serde_json::to_value(engine.snapshot().unwrap()).unwrap();
	let resolved_before = resolved.load(Ordering::Relaxed);
	assert!(choose(&mut engine, Some("known_module")).is_err());
	assert_eq!(resolved.load(Ordering::Relaxed), resolved_before);
	assert_eq!(
		serde_json::to_value(engine.snapshot().unwrap()).unwrap(),
		before
	);
}

#[test]
fn existing_unknown_module_remains_editable_and_can_be_cleared_then_restored() {
	let document = scene(vec![]);
	let mut project = ProjectData::for_scene("Imported", "scenes/main.json", &document).unwrap();
	project.gameplay_module = Some("unavailable_module".into());
	let mut engine = EngineBuilder::new(document, project.clone())
		.build()
		.unwrap();
	project.name = "Renamed project".into();
	execute(
		&mut engine,
		Command::SetProject {
			project: project.clone(),
		},
	)
	.unwrap();
	assert_eq!(engine.session().project(), &project);
	assert!(execute(&mut engine, Command::StartPlay).is_err());
	choose(&mut engine, None).unwrap();
	execute(&mut engine, Command::Undo).unwrap();
	assert_eq!(engine.session().project(), &project);
	execute(&mut engine, Command::Redo).unwrap();
	assert!(engine.session().project().gameplay_module.is_none());
	execute(&mut engine, Command::StartPlay).unwrap();
	assert!(engine.session().is_playing());
}

#[test]
fn capabilities_from_before_the_module_catalog_remain_readable() {
	let capabilities: EditorCapabilitiesDto = serde_json::from_value(serde_json::json!({
		"protocol_version": 2,
		"scene_schema_version": 3,
		"commands": [],
		"max_selection": 4096,
		"max_history_entries": 128,
		"history_budget_bytes": 16777216,
		"embedded_viewport_supported": true
	}))
	.unwrap();
	assert!(capabilities.gameplay_modules.is_empty());
	assert_eq!(
		serde_json::to_value(&capabilities).unwrap()["gameplay_modules"],
		serde_json::json!([])
	);
}
