use super::*;
use runtime::{
	assets::project_assets::{load_catalog, prepare_input_actions, prepare_move},
	components::{ComponentData, InputActionsRef},
	input::InputActionMap,
};

#[test]
fn assigned_input_actions_survive_rename_and_build_from_the_saved_project() {
	let fixture = Fixture::new();
	let mut workspace = fixture.open();
	let map = InputActionMap::starter();
	let created = prepare_input_actions(&fixture.0, "controls.input-actions.json", None, &map)
		.unwrap()
		.publish()
		.unwrap();
	run(&mut workspace, Command::CreatePlayer);
	let snapshot = workspace.backend.snapshot().unwrap();
	let key = EntityKey::new(&snapshot.selection[0]).unwrap();
	let authored = workspace.backend.session().authored_document();
	let player = authored
		.entities
		.iter()
		.find(|entity| entity.key == key)
		.unwrap();
	let mut component = player
		.component_records()
		.into_iter()
		.find(|record| matches!(&record.data, ComponentData::PlayerController { .. }))
		.unwrap();
	let ComponentData::PlayerController { settings } = &mut component.data else {
		unreachable!()
	};
	settings.input = InputActionsRef::Asset {
		asset_id: created.id.clone(),
	};
	run(
		&mut workspace,
		Command::ReplaceComponent {
			key: key.as_str().into(),
			component,
		},
	);
	run(
		&mut workspace,
		Command::SetLocalPlayer {
			key: Some(key.clone()),
		},
	);
	workspace
		.save_all(workspace.backend.session().revision())
		.unwrap();
	let renamed = prepare_move(&fixture.0, &created.id, "movement.input-actions.json")
		.unwrap()
		.publish()
		.unwrap();
	assert_eq!(created.id, renamed.id);
	let prepared = workspace
		.prepare_build(workspace.backend.session().revision())
		.unwrap();
	let output = fixture.0.parent().unwrap().join("input-package");
	fs::create_dir(&output).unwrap();
	let staged = compiler::stage_prepared_project(&prepared, output.to_str().unwrap()).unwrap();
	let root = staged.path().join("assets");
	assert!(!root.join(&created.path).exists());
	let catalog = load_catalog(&root).unwrap();
	let asset = catalog.by_id(&created.id).unwrap();
	assert_eq!(asset.path, renamed.path);
	assert_eq!(
		fs::read(root.join(&renamed.path)).unwrap(),
		fs::read(fixture.0.join(&renamed.path)).unwrap()
	);
	assert_eq!(
		runtime::assets::project_assets::active_input_actions(
			&root,
			&runtime::scene_io::load_data(&root.join("scenes/main.json"))
				.unwrap()
				.data,
		)
		.unwrap(),
		Some(map)
	);
	fs::remove_file(fixture.0.join(&renamed.path)).unwrap();
	let error = workspace
		.prepare_build(workspace.backend.session().revision())
		.err()
		.unwrap();
	assert!(
		error.contains("scenes/main.json") && error.contains(key.as_str()),
		"{error}"
	);
}
