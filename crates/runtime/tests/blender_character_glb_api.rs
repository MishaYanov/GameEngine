use std::{collections::HashSet, env, fs, path::PathBuf};

use runtime::{
	asset_catalog::AssetId,
	assets::project_assets::{load_catalog, prepare_import, prepare_reimport_with_references},
};

struct ImportedFixture(PathBuf);

impl Drop for ImportedFixture {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

#[test]
#[ignore = "generate the fixtures with Blender using integrations/blender/build_character_fixture.py"]
fn blender_character_reimport_preserves_clip_and_joint_bindings_atomically() {
	let fixtures = PathBuf::from(env::var_os("GAME_ENGINE_CHARACTER_FIXTURE_ROOT").unwrap())
		.canonicalize()
		.unwrap();
	let root = ImportedFixture(fixtures.join(format!("managed-{}", AssetId::generate().as_str())));
	fs::create_dir(&root.0).unwrap();
	let initial = prepare_import(
		&root.0,
		&fixtures.join("character-fixture.glb"),
		"character.glb",
	)
		.unwrap()
		.publish()
		.unwrap();
	let source = bridge::animation_asset::inspect(&root.0, &initial.id)
		.unwrap()
		.source;
	let before = fs::read(root.0.join("character.glb")).unwrap();
	let missing = prepare_reimport_with_references(
		&root.0,
		&initial.id,
		Some(&fixtures.join("character-fixture-missing-clip.glb")),
		&initial.fingerprint,
		&HashSet::new(),
		&HashSet::new(),
	);
	let message = missing.err().unwrap();
	assert!(
		message.contains("Character clip") && message.contains("was removed"),
		"{message}"
	);
	assert_eq!(fs::read(root.0.join("character.glb")).unwrap(), before);
	assert_eq!(
		load_catalog(&root.0).unwrap().by_id(&initial.id),
		Some(&initial)
	);

	let mesh_id = initial.meshes[0].identity.as_ref().unwrap().id.clone();
	let updated = prepare_reimport_with_references(
		&root.0,
		&initial.id,
		Some(&fixtures.join("character-fixture-revised.glb")),
		&initial.fingerprint,
		&HashSet::new(),
		&HashSet::from([mesh_id.clone()]),
	)
		.unwrap()
		.publish()
		.unwrap();
	assert_eq!(updated.meshes[0].identity.as_ref().unwrap().id, mesh_id);
	let animation = bridge::animation_asset::load(&root.0, &source).unwrap();
	assert_eq!(animation.resolve_clip_index(&source, 0).unwrap(), 1);
	assert_eq!(animation.resolve_clip_index(&source, 1).unwrap(), 0);
	for id in source.character.unwrap().joints.values() {
		assert!(animation.joint_node(id).is_some());
	}
}
