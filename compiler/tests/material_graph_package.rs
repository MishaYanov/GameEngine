use std::{fs, path::PathBuf};

use engine_core::{
	EntityKey, SceneData,
	asset_catalog::{
		AssetCatalog, AssetId, AssetKind, CATALOG_FILE, MeshSignature, PersistentAsset,
	},
	command::{EditorCommandDto, EditorCommandRequest},
	editor::EditorSession,
	material_graph::{MaterialGraph, MaterialGraphErrorCode},
	project::ProjectData,
	scene_data::{CURRENT_SCHEMA_VERSION, EntityData, MaterialOverrideData},
};
use serde_json::json;
use sha2::{Digest, Sha256};

const CUBE: &[u8] = include_bytes!("../../apps/game/assets/props/cube.glb");

struct Fixture(PathBuf);

impl Drop for Fixture {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

fn graph() -> MaterialGraph {
	serde_json::from_value(json!({
		"version": 1, "domain": "shader",
		"nodes": [
			{"id": "color", "kind": "shader.color", "position": {"x": 0, "y": 0}, "values": {"red": 0.5, "green": 0.25, "blue": 0.75}},
			{"id": "value", "kind": "value", "position": {"x": 0, "y": 200}, "values": {"value": 0.5}},
			{"id": "multiply", "kind": "shader.multiply", "position": {"x": 200, "y": 0}, "values": {"factor": 0.25}},
			{"id": "output", "kind": "shader.output", "position": {"x": 400, "y": 0}, "values": {}}
		],
		"connections": [
			{"id": "color-wire", "from": {"node": "color", "port": "color"}, "to": {"node": "multiply", "port": "value"}},
			{"id": "scalar-wire", "kind": "data", "from": {"node": "value", "port": "value"}, "to": {"node": "multiply", "port": "factor"}},
			{"id": "output-wire", "from": {"node": "multiply", "port": "result"}, "to": {"node": "output", "port": "color"}}
		]
	})).unwrap()
}

#[test]
fn compile_material_graph_points_to_invalid_nodes_ports_and_cycles() {
	let source = graph();
	assert_eq!(
		source.compile_base_color().unwrap(),
		[0.25, 0.125, 0.375, 1.0]
	);
	let mut missing_color = source.clone();
	missing_color.connections.pop();
	let error = missing_color.compile_base_color().unwrap_err();
	assert_eq!(error.code, MaterialGraphErrorCode::MissingInput);
	assert_eq!(error.node.as_deref(), Some("output"));
	assert_eq!(error.port.as_deref(), Some("color"));

	let mut time = source.clone();
	time.nodes[1].kind = "shader.time".into();
	let error = time.compile_base_color().unwrap_err();
	assert_eq!(error.code, MaterialGraphErrorCode::UnsupportedNode);
	assert_eq!(error.node.as_deref(), Some("value"));

	let mut wrong_type = source.clone();
	wrong_type.connections[1].from.node = "color".into();
	wrong_type.connections[1].from.port = "color".into();
	let error = wrong_type.compile_base_color().unwrap_err();
	assert_eq!(error.code, MaterialGraphErrorCode::Connection);
	assert_eq!(error.port.as_deref(), Some("factor"));

	let mut cycle = source;
	cycle.connections[0].from.node = "multiply".into();
	cycle.connections[0].from.port = "result".into();
	let error = cycle.compile_base_color().unwrap_err();
	assert_eq!(error.code, MaterialGraphErrorCode::Cycle);
	assert_eq!(error.node.as_deref(), Some("multiply"));
}

#[test]
fn authored_material_graph_reaches_the_exported_scene_through_normal_editor_command() {
	let fixture = Fixture(std::env::temp_dir().join(format!(
		"engine-material-graph-{}",
		AssetId::generate().as_str()
	)));
	let assets = fixture.0.join("assets");
	let output = fixture.0.join("exports");
	fs::create_dir_all(assets.join("scenes")).unwrap();
	fs::create_dir(&output).unwrap();
	fs::write(assets.join("cube.glb"), CUBE).unwrap();

	let model = PersistentAsset {
		id: AssetId::generate(),
		kind: AssetKind::Model,
		path: "cube.glb".into(),
		revision: 1,
		fingerprint: format!("{:x}", Sha256::digest(CUBE)),
		bytes: CUBE.len() as u64,
		aliases: Vec::new(),
		source_path: None,
		meshes: vec![MeshSignature {
			index: 0,
			name: "cube".into(),
			primitives: 1,
			identity: None,
		}],
		dependencies: Vec::new(),
	};
	let scene = SceneData {
		schema_version: CURRENT_SCHEMA_VERSION,
		bindings: Default::default(),
		entities: vec![
			EntityData::new(EntityKey::new("cube").unwrap(), "Cube", "cube.glb")
				.with_asset_id(Some(model.id.clone())),
		],
		camera: Default::default(),
		default_game_camera: None,
		environment: Default::default(),
		lights: Vec::new(),
		organization: Default::default(),
	};
	let catalog = AssetCatalog {
		schema_version: 1,
		assets: vec![model],
	};
	fs::write(assets.join(CATALOG_FILE), catalog.to_json().unwrap()).unwrap();

	let project = ProjectData::for_scene("Material Graph", "scenes/main.json", &scene).unwrap();
	let mut session = EditorSession::for_project(scene, project.clone()).unwrap();
	let material = MaterialOverrideData {
		base_color_factor: [0.25, 0.125, 0.375, 0.75],
		source_graph: Some(Box::new(graph())),
	};
	let mut forged_material = material.clone();
	forged_material.base_color_factor[0] = 0.0;
	let request = |material| EditorCommandRequest {
		expected_revision: 0,
		command: EditorCommandDto::SetMaterial {
			key: "cube".into(),
			material: Some(material),
		},
	};
	assert!(
		session
			.execute(request(forged_material))
			.unwrap_err()
			.contains("differs")
	);
	session.execute(request(material.clone())).unwrap();
	let authored = session.authored_document();
	let authored_json = authored.to_json().unwrap();
	assert_eq!(
		SceneData::from_json(&authored_json).unwrap().entities[0].material,
		Some(material.clone())
	);
	fs::write(assets.join("scenes/main.json"), authored_json).unwrap();

	let prepared = compiler::prepare_project(&project, &assets).unwrap();
	let staged = compiler::stage_prepared_project(&prepared, output.to_str().unwrap()).unwrap();
	let player_fixture = std::env::current_exe().unwrap();
	let installed = compiler::complete_staged_project_with_player(staged, &player_fixture).unwrap();
	let exported = fs::read_to_string(installed.path().join("assets/scenes/main.json")).unwrap();
	let exported = SceneData::from_json(&exported).unwrap();
	assert_eq!(exported.entities[0].material, Some(material));
}
