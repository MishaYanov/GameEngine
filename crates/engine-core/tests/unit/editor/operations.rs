use super::*;

#[test]
fn selection_operations_preserve_hierarchy_error_order_before_mutation_or_noop() {
	let camera = crate::Camera::default();
	let key = |value: &str| EntityKey::new(value).unwrap();
	let original = SceneData {
		schema_version: crate::scene_data::CURRENT_SCHEMA_VERSION,
		organization: Default::default(),
		default_game_camera: None,
		bindings: Default::default(),
		entities: vec![
			EntityData::new(key("a"), "A", "model.glb"),
			EntityData::new(key("b"), "B", "model.glb"),
		],
		camera: CameraData {
			follow: None,
			orbit: None,
			position: camera.position,
			yaw: camera.yaw,
			pitch: camera.pitch,
			vertical_fov: camera.vertical_fov,
			near_plane: camera.near,
			far_plane: camera.far,
		},
		lights: vec![],
	};
	let mut duplicate = original.clone();
	duplicate.entities[1].key = key("a");
	duplicate.entities[0].parent = Some(key("missing-parent"));
	let mut missing_parent = original.clone();
	missing_parent.entities[0].parent = Some(key("missing-parent"));
	let mut cycle = original.clone();
	cycle.entities[0].parent = Some(key("b"));
	cycle.entities[1].parent = Some(key("a"));
	for (data, selected, expected) in [
		(
			duplicate,
			"missing-selection",
			"Scene contains duplicate entity keys",
		),
		(
			missing_parent,
			"missing-selection",
			"Parent \"missing-parent\" does not exist",
		),
		(
			cycle.clone(),
			"missing-selection",
			"Selected entity \"missing-selection\" no longer exists",
		),
		(cycle, "a", "Scene hierarchy contains a parent cycle"),
	] {
		for command in [
			EditorCommandDto::DuplicateSelection { offset: [0.0; 3] },
			EditorCommandDto::DeleteSelection,
			EditorCommandDto::TranslateSelection {
				delta: [0.0; 3],
				snap: false,
			},
		] {
			let mut candidate = data.clone();
			let mut selection = vec![key(selected)];
			let error = apply(
				&mut candidate,
				&mut selection,
				&SnapSettingsDto::default(),
				&HashMap::new(),
				command,
			)
			.unwrap_err();
			assert_eq!(error, expected);
			assert_eq!(candidate, data);
			assert_eq!(selection, vec![key(selected)]);
		}
	}
}
