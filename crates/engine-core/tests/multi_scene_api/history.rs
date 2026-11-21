use super::*;

#[test]
fn cross_scene_undo_and_redo_restore_patch_selection_and_retain_the_outgoing_selection() {
	let (mut engine, first, second) = fixture();
	select(&mut engine, &["actor"]);
	run(&mut engine, Command::DeleteSelection);
	assert!(engine.session().selection().is_empty());
	switch(&mut engine, &second);
	select(&mut engine, &["actor"]);

	run(&mut engine, Command::Undo);
	assert_eq!(engine.session().active_scene_id(), &first);
	assert_eq!(engine.session().selection(), &[key("actor")]);
	assert_eq!(
		engine.snapshot().unwrap().document.entities[0].key,
		key("actor")
	);
	switch(&mut engine, &second);
	assert_eq!(engine.session().selection(), &[key("actor")]);

	run(&mut engine, Command::Redo);
	assert_eq!(engine.session().active_scene_id(), &first);
	assert!(engine.session().selection().is_empty());
	assert!(engine.snapshot().unwrap().document.entities.is_empty());
	switch(&mut engine, &second);
	assert_eq!(engine.session().selection(), &[key("actor")]);
	switch(&mut engine, &first);
	assert!(engine.session().selection().is_empty());
}
