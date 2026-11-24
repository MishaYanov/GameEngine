use std::{collections::HashSet, env, fs, path::PathBuf};

use runtime::{
	asset_catalog::AssetId,
	asset_registry::AssetRef,
	assets::{
		gltf_loader::load_scene,
		project_assets::{prepare_import, prepare_reimport_with_references, resolve_asset},
	},
};

struct ImportedFixture(PathBuf);

impl Drop for ImportedFixture {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

#[test]
#[ignore = "generate the fixture with Blender using integrations/blender/build_static_fixture.py"]
fn blender_export_reimports_a_stable_reference_and_preserves_the_static_profile() {
	let root = PathBuf::from(env::var_os("GAME_ENGINE_BLENDER_FIXTURE_ROOT").unwrap())
		.canonicalize()
		.unwrap();
	let first = root.join("static-fixture.glb");
	let second = root.join("static-fixture-revised.glb");
	let asset_root =
		ImportedFixture(root.join(format!("managed-{}", AssetId::generate().as_str())));
	fs::create_dir(&asset_root.0).unwrap();
	let initial = prepare_import(&asset_root.0, &first, "fixture.glb")
		.unwrap()
		.publish()
		.unwrap();
	let body = initial
		.meshes
		.iter()
		.find_map(|mesh| {
			let identity = mesh.identity.as_ref()?;
			(identity.source_key.as_deref() == Some("fixture-body")).then_some(identity)
		})
		.unwrap();
	let reference = AssetRef::new("fixture.glb", None)
		.unwrap()
		.with_asset_id(Some(initial.id.clone()))
		.with_mesh_id(Some(body.id.clone()));
	let previous = resolve_asset(&asset_root.0, &reference)
		.unwrap()
		.mesh_index
		.unwrap();
	let first_scene = load_scene(asset_root.0.join("fixture.glb")).unwrap();
	assert_eq!(first_scene.mesh_bounds[previous].unwrap().max()[0], 2.0);
	let parent = first_scene
		.nodes
		.iter()
		.find(|node| node.name == "FixtureRoot")
		.unwrap();
	assert_eq!(parent.local_transform.position, [3.5, 7.5, -5.5]);
	let cap = first_scene
		.nodes
		.iter()
		.find(|node| node.name == "FixtureCap")
		.unwrap();
	assert_eq!(cap.parent_node_index, Some(parent.node_index));
	assert_eq!(cap.local_transform.position, [1.0, 3.0, 2.0]);
	assert!(cap.local_transform.scale.iter().product::<f32>() < 0.0);
	assert!(
		first_scene
			.materials
			.iter()
			.any(|material| material.base_color_image_index.is_some())
	);

	let updated = prepare_reimport_with_references(
		&asset_root.0,
		&initial.id,
		Some(&second),
		&initial.fingerprint,
		&HashSet::new(),
		&HashSet::from([body.id.clone()]),
	)
	.unwrap()
	.publish()
	.unwrap();
	assert_eq!(updated.id, initial.id);
	assert_eq!(updated.revision, initial.revision + 1);
	assert_ne!(updated.fingerprint, initial.fingerprint);
	let resolved = resolve_asset(&asset_root.0, &reference).unwrap();
	assert_eq!(resolved.mesh_id.as_ref(), Some(&body.id));
	assert_ne!(
		updated
			.mesh_by_id(&body.id)
			.unwrap()
			.identity
			.as_ref()
			.unwrap()
			.geometry_sha256,
		body.geometry_sha256
	);
	let scene = load_scene(asset_root.0.join("fixture.glb")).unwrap();
	assert_eq!(
		scene.mesh_bounds[resolved.mesh_index.unwrap()]
			.unwrap()
			.max()[0],
		2.75
	);
}
