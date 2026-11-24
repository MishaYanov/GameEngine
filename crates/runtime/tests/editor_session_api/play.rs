use super::support::{close, entity, key, scene};
use super::{fixture, reject, run, select};
use bridge::editor::EditorCommandDto as Command;
use runtime::editor::EditorSession;

#[test]
fn play_ticks_copy_authored_state_without_invalidating_command_revision_and_pause_discards_backlog()
{
	let mut s = fixture();
	select(&mut s, &["a"]);
	let authored = s.authored_document();
	run(&mut s, Command::StartPlay);
	assert!(std::sync::Arc::ptr_eq(&s.render_document(), &authored));
	run(
		&mut s,
		Command::SetPlayInput {
			movement: [3.0, 4.0, 0.0],
		},
	);
	let revision = s.revision();
	let scene_revision = s.scene_revision();
	for _ in 0..6 {
		s.tick(1.0 / 60.0).unwrap();
	}
	assert_eq!(s.revision(), revision);
	assert!(s.scene_revision() > scene_revision);
	close(s.render_document().entities[0].transform.position[0], 0.12);
	close(s.render_document().entities[0].transform.position[1], 0.16);
	assert_eq!(s.authored_document(), authored);
	assert!(!s.snapshot().unwrap().dirty);
	reject(
		&mut s,
		Command::Rename {
			key: "a".into(),
			name: "Forbidden".into(),
		},
	);
	run(&mut s, Command::SetPaused { paused: true });
	let paused = s.render_document();
	s.tick(60.0).unwrap();
	assert_eq!(s.render_document(), paused);
	run(&mut s, Command::SetPaused { paused: false });
	s.tick(0.1).unwrap();
	assert_eq!(s.render_document(), paused);
	run(
		&mut s,
		Command::SetPlayInput {
			movement: [1.0, 0.0, 0.0],
		},
	);
	s.tick(1.0 / 120.0).unwrap();
	assert_eq!(s.render_document(), paused);
	run(&mut s, Command::StopPlay);
	assert_eq!(s.render_document(), authored);
	assert!(s.tick(f64::NAN).is_err());
	assert_eq!(s.authored_document(), authored);
}

#[test]
fn paused_play_transforms_apply_once_and_undo_as_authored_changes() {
	let mut session = fixture();
	let authored = session.authored_document();
	run(&mut session, Command::StartPlay);
	run(
		&mut session,
		Command::SetPlayInput {
			movement: [1.0, 0.0, 0.0],
		},
	);
	session.tick(1.0 / 30.0).unwrap();
	assert!(session.review_play_transforms().is_err());
	run(&mut session, Command::SetPaused { paused: true });

	let review = session.review_play_transforms().unwrap();
	assert_eq!(review.changes.len(), 1);
	let changed = &review.changes[0];
	assert_eq!(changed.key, key("a"));
	assert_eq!(session.authored_document(), authored);
	let command = |source: u64, keys: Vec<_>| Command::ApplyPlayTransforms {
		run_id: review.run_id.clone(),
		scene_revision: source,
		keys,
	};
	reject(
		&mut session,
		command(review.scene_revision - 1, vec![key("a")]),
	);
	reject(
		&mut session,
		command(review.scene_revision, vec![key("a"), key("b")]),
	);
	assert!(session.is_paused());
	assert_eq!(session.authored_document(), authored);

	run(&mut session, command(review.scene_revision, vec![key("a")]));
	assert!(!session.is_playing());
	assert_eq!(
		session.authored_document().entities[0].transform.position,
		changed.after.position
	);
	assert!(session.snapshot().unwrap().dirty);
	run(&mut session, Command::Undo);
	assert_eq!(session.authored_document(), authored);
	assert!(!session.snapshot().unwrap().dirty);
}

#[test]
fn play_tick_demand_requires_target_and_movement_and_motion_error_pauses_without_partial_edit() {
	let mut parent = entity("parent", [0.0; 3]);
	parent.transform.scale = [0.0, 1.0, 1.0];
	let child = entity("child", [0.0; 3]).with_parent(Some(key("parent")));
	let data = scene(vec![parent, child]);
	let mut project =
		engine_core::project::ProjectData::for_scene("Test project", "scenes/main.json", &data)
			.unwrap();
	project.add_behavior(engine_core::project::BehaviorSpec::ControlledMove {
		entity: key("child"),
		speed: 2.0,
		space: engine_core::project::BehaviorSpace::World,
	});
	let mut s = EditorSession::for_project(data, project).unwrap();
	assert!(!s.needs_tick());
	run(&mut s, Command::StartPlay);
	assert!(!s.needs_tick());
	run(
		&mut s,
		Command::SetPlayInput {
			movement: [1.0, 0.0, 0.0],
		},
	);
	assert!(s.needs_tick());
	select(&mut s, &["parent"]); // UI selection does not control the configured actor.
	assert!(s.needs_tick());
	let before = s.render_document();
	let revision = s.revision();
	assert!(s.tick(0.1).unwrap_err().contains("paused"));
	assert!(s.is_paused());
	assert!(!s.needs_tick());
	assert_eq!(s.render_document(), before);
	assert_eq!(s.revision(), revision);
	s.tick(1.0).unwrap();
	assert_eq!(s.render_document(), before);
}

#[test]
fn compatibility_session_does_not_run_another_scenes_behavior_for_an_overlapping_entity_key() {
	use engine_core::{
		project::{BehaviorSpec, ProjectData, SceneBehavior},
		scene_catalog::{SceneDescriptor, SceneId},
	};

	let data = scene(vec![entity("actor", [0.0; 3])]);
	let mut project =
		ProjectData::for_scene("Scoped behaviors", "scenes/main.json", &data).unwrap();
	let secondary_id = SceneId::new("secondary").unwrap();
	project
		.scenes
		.insert(SceneDescriptor::new(secondary_id.clone(), "scenes/secondary.json").unwrap())
		.unwrap();
	project.behaviors.push(SceneBehavior {
		scene_id: secondary_id,
		behavior: BehaviorSpec::Spin {
			entity: key("actor"),
			angular_velocity: [0.0, 2.0, 0.0],
		},
	});
	let mut session = EditorSession::for_project(data, project).unwrap();
	let authored = session.authored_document();
	run(&mut session, Command::StartPlay);
	for _ in 0..6 {
		session.tick(1.0 / 60.0).unwrap();
	}
	assert_eq!(session.render_document(), authored);
	assert!(!session.needs_tick());
}
