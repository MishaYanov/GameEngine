use super::*;
use crate::{PreparedProject, stage_prepared_project};

#[test]
fn duplicate_uri_edges_read_once_and_later_source_changes_still_reject_staging() {
	let root = std::env::temp_dir().join(format!("export-uri-{}", AssetId::generate().as_str()));
	fs::create_dir(&root).unwrap();
	let root = root.canonicalize().unwrap();
	let project =
		ProjectData::load(Path::new(env!("CARGO_MANIFEST_DIR")).join("../apps/game/project.json"))
			.unwrap();
	let model = serde_json::json!({
		"asset": { "version": "2.0" },
		"buffers": (0..128).map(|index| serde_json::json!({ "uri": if cfg!(windows) && index % 2 == 1 { "SHARED.BIN" } else { "shared.bin" }, "byteLength": 36 })).collect::<Vec<_>>(),
		"bufferViews": [{ "buffer": 0, "byteLength": 36 }],
		"accessors": [{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0,0,0], "max": [0,0,0] }],
		"meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 } }] }]
	});
	fs::write(root.join("model.gltf"), model.to_string()).unwrap();
	fs::write(root.join("shared.bin"), [0u8; 36]).unwrap();
	let mut closure = Closure {
		root: &root,
		project: &project,
		catalog: AssetCatalog::default(),
		files: BTreeMap::new(),
		ids: HashSet::new(),
		models: HashMap::new(),
		animations: HashMap::new(),
		animation_bytes: 0,
		prefabs: HashSet::new(),
		prefab_stack: HashSet::new(),
		generated_files: BTreeMap::new(),
		generated_bytes: 0,
		linked_sources: HashSet::new(),
		resolved_paths: HashMap::new(),
		bytes: 0,
		depth: 0,
		uri_edges: 0,
		file_reads: 0,
	};
	closure.model(Path::new("model.gltf"), None).unwrap();
	#[cfg(windows)]
	closure.model(Path::new("MODEL.GLTF"), None).unwrap();
	assert_eq!(closure.uri_edges, 128);
	assert_eq!(
		closure.file_reads, 2,
		"one model read and one shared dependency read"
	);
	assert_eq!(closure.files.len(), 2);
	assert_eq!(
		closure.models.len(),
		1,
		"case aliases reuse one parsed model"
	);
	let frozen: Vec<_> = closure.files.into_values().collect();
	let prepared = PreparedProject {
		project: project.clone(),
		asset_root: root.clone(),
		files: frozen.clone(),
		source_files: frozen,
		generated_files: BTreeMap::new(),
		total_bytes: closure.bytes,
		catalog_source: None,
		catalog_output: None,
	};
	fs::write(root.join("shared.bin"), [1u8; 36]).unwrap();
	assert!(
		prepared
			.validate_inputs()
			.unwrap_err()
			.contains("shared.bin")
	);
	assert!(stage_prepared_project(&prepared, root.to_str().unwrap()).is_err());
	assert!(!root.join(&prepared.project.name).exists());
	fs::remove_dir_all(root).unwrap();
}

#[test]
fn uri_work_budget_rejects_before_reading_another_dependency() {
	let project =
		ProjectData::load(Path::new(env!("CARGO_MANIFEST_DIR")).join("../apps/game/project.json"))
			.unwrap();
	let mut closure = Closure {
		root: Path::new(env!("CARGO_MANIFEST_DIR")),
		project: &project,
		catalog: AssetCatalog::default(),
		files: BTreeMap::new(),
		ids: HashSet::new(),
		models: HashMap::new(),
		animations: HashMap::new(),
		animation_bytes: 0,
		prefabs: HashSet::new(),
		prefab_stack: HashSet::new(),
		generated_files: BTreeMap::new(),
		generated_bytes: 0,
		linked_sources: HashSet::new(),
		resolved_paths: HashMap::new(),
		bytes: 0,
		depth: 0,
		uri_edges: MAX_URI_EDGES,
		file_reads: 0,
	};
	assert!(
		closure
			.capture_uri(Path::new("model.gltf"), "shared.bin")
			.unwrap_err()
			.contains("100000 URI edges")
	);
	assert_eq!(closure.file_reads, 0);
	assert!(closure.files.is_empty());
}
