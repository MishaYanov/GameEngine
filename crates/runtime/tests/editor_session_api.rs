use bridge::editor::{
	EditorCommandDto as Command, EditorCommandRequest, EditorSnapshotDto, TransformDto,
	TransformEditDto,
};
use runtime::editor::EditorSession;
use support::{entity, key, scene};

#[path = "editor_session_api/camera.rs"]
mod camera;
#[path = "editor_session_api/history.rs"]
mod history;
#[path = "editor_session_api/play.rs"]
mod play;
#[path = "editor_session_api/prefabs.rs"]
mod prefabs;
mod support;
#[path = "editor_session_api/transforms.rs"]
mod transforms;

fn run(session: &mut EditorSession, command: Command) -> EditorSnapshotDto {
	session
		.execute(EditorCommandRequest {
			expected_revision: session.revision(),
			command,
		})
		.unwrap()
}
fn reject(session: &mut EditorSession, command: Command) -> String {
	let before = serde_json::to_value(session.snapshot().unwrap()).unwrap();
	let error = session
		.execute(EditorCommandRequest {
			expected_revision: session.revision(),
			command,
		})
		.unwrap_err();
	assert_eq!(
		serde_json::to_value(session.snapshot().unwrap()).unwrap(),
		before,
		"failed command mutated session: {error}"
	);
	error
}
fn select(session: &mut EditorSession, keys: &[&str]) {
	run(
		session,
		Command::Select {
			keys: keys.iter().map(|s| (*s).into()).collect(),
		},
	);
}
fn fixture() -> EditorSession {
	let data = scene(vec![entity("a", [0.0; 3]), entity("b", [2.0, 0.0, 0.0])]);
	let mut project =
		engine_core::project::ProjectData::for_scene("Test project", "scenes/main.json", &data)
			.unwrap();
	project.add_behavior(engine_core::project::BehaviorSpec::ControlledMove {
		entity: key("a"),
		speed: 2.0,
		space: engine_core::project::BehaviorSpace::World,
	});
	EditorSession::for_project(data, project).unwrap()
}
fn transform(key: &str, position: [f32; 3]) -> TransformEditDto {
	TransformEditDto {
		key: key.into(),
		transform: TransformDto {
			position,
			..Default::default()
		},
	}
}
