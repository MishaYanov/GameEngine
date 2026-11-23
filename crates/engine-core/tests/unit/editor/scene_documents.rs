use super::*;
use crate::scene_data::{CURRENT_SCHEMA_VERSION, CameraData, EntityData};

fn document(name: &str) -> SceneData {
	let camera = crate::Camera::default();
	SceneData {
		schema_version: CURRENT_SCHEMA_VERSION,
		organization: Default::default(),
		default_game_camera: None,
		environment: Default::default(),
		bindings: Default::default(),
		entities: vec![EntityData::new(
			EntityKey::new("actor").unwrap(),
			name,
			"models/actor.glb",
		)],
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
	}
}

fn serialized_cost(data: &SceneData) -> usize {
	data.to_json().unwrap().len() * 4 + std::mem::size_of::<SceneData>()
}

#[test]
fn counted_cost_matches_pretty_json_and_validation_errors() {
	let mut data = document("Actör \"quoted\" \\ name");
	data.bindings
		.insert("主角".into(), EntityKey::new("actor").unwrap());
	data.entities[0].transform.position = [-0.0, 1.0e-5, 1.0e12];
	assert_eq!(document_cost(&data).unwrap(), serialized_cost(&data));
	data.schema_version = 0;
	assert_eq!(document_cost(&data), data.to_json().map(|_| 0));
	let mut cache = DocumentCostCache::default();
	assert!(cache.cost(&Arc::new(data)).is_err());
	assert!(cache.entries.is_empty());
}

#[test]
fn budget_counts_current_saved_and_replacement_documents_exactly() {
	let active = SceneId::new("main").unwrap();
	let second = SceneId::new("second").unwrap();
	let data = Arc::new(document("active"));
	let inactive = Arc::new(document("inactive"));
	let mut replacement = document("replacement has a longer name");
	let drafts = HashMap::from([(
		second.clone(),
		SceneDraft {
			data: Arc::clone(&inactive),
			saved: Arc::clone(&inactive),
			saved_path: Some("scenes/second.json".into()),
			selection: vec![],
		},
	)]);
	let mut cache = DocumentCostCache::default();
	let shared = serialized_cost(&data) + serialized_cost(&inactive);
	assert_eq!(
		EditorSession::document_budget(&active, &data, &data, &drafts, None, &mut cache).unwrap(),
		shared
	);
	for id in [&active, &second] {
		assert_eq!(
			EditorSession::document_budget(
				&active,
				&data,
				&data,
				&drafts,
				Some((id, &replacement)),
				&mut cache,
			)
			.unwrap(),
			shared + serialized_cost(&replacement)
		);
	}
	replacement.entities[0].name.push_str(" changed again");
	assert_eq!(
		EditorSession::document_budget(
			&active,
			&data,
			&data,
			&drafts,
			Some((&active, &replacement)),
			&mut cache,
		)
		.unwrap(),
		shared + serialized_cost(&replacement)
	);
	let separate_saved = Arc::new((*data).clone());
	assert_eq!(
		EditorSession::document_budget(&active, &data, &separate_saved, &drafts, None, &mut cache,)
			.unwrap(),
		shared + serialized_cost(&separate_saved)
	);
}

#[test]
fn cost_cache_replaces_identities_and_prunes_externally_held_retired_documents() {
	let active = SceneId::new("main").unwrap();
	let mut data = Arc::new(document("before"));
	let retired = Arc::clone(&data);
	let mut cache = DocumentCostCache::default();
	let drafts = HashMap::new();
	EditorSession::document_budget(&active, &data, &data, &drafts, None, &mut cache).unwrap();
	assert_eq!(Arc::strong_count(&data), 2);
	assert_eq!(cache.entries.len(), 1);
	Arc::make_mut(&mut data).entities[0].name = "a substantially longer replacement".into();
	assert_ne!(serialized_cost(&data), serialized_cost(&retired));
	assert_eq!(
		EditorSession::document_budget(&active, &data, &data, &drafts, None, &mut cache).unwrap(),
		serialized_cost(&data)
	);
	assert_eq!(cache.entries.len(), 1);
	assert!(
		!cache
			.entries
			.contains_key(&(Arc::as_ptr(&retired) as usize))
	);
	assert_eq!(Arc::strong_count(&retired), 1);
	assert_eq!(Arc::strong_count(&data), 1);
	let weak = Arc::downgrade(&data);
	drop(data);
	assert!(
		weak.upgrade().is_none(),
		"cache must not retain document contents"
	);
}

#[test]
fn retained_budget_still_rejects_the_next_inactive_document_after_cached_counting() {
	let active = SceneId::new("main").unwrap();
	let mut data = document("large");
	data.entities = (0..2048)
		.map(|index| {
			EntityData::new(
				EntityKey::new(format!("actor-{index}")).unwrap(),
				"x".repeat(1024),
				"models/actor.glb",
			)
		})
		.collect();
	let data = Arc::new(data);
	let cost = serialized_cost(&data);
	let allowed_count = MAX_RETAINED_DOCUMENT_BYTES / cost;
	assert!((2..MAX_PROJECT_SCENES).contains(&allowed_count));
	let mut drafts: HashMap<_, _> = (1..allowed_count)
		.map(|index| {
			(
				SceneId::new(format!("scene-{index}")).unwrap(),
				SceneDraft {
					data: Arc::clone(&data),
					saved: Arc::clone(&data),
					saved_path: Some(format!("scenes/{index}.json")),
					selection: vec![],
				},
			)
		})
		.collect();
	let mut cache = DocumentCostCache::default();
	assert_eq!(
		EditorSession::document_budget(&active, &data, &data, &drafts, None, &mut cache).unwrap(),
		allowed_count * cost
	);
	let extra = SceneId::new("extra").unwrap();
	drafts.insert(extra.clone(), drafts.values().next().unwrap().clone());
	assert_eq!(
		EditorSession::document_budget(&active, &data, &data, &drafts, None, &mut cache)
			.unwrap_err(),
		"Retained scene document byte budget exceeded"
	);
	drafts.remove(&extra);
	assert_eq!(
		EditorSession::document_budget(&active, &data, &data, &drafts, None, &mut cache).unwrap(),
		allowed_count * cost
	);
}

#[test]
fn failed_post_preview_restore_keeps_document_and_history_for_retry() {
	let active = SceneId::new("main").unwrap();
	let second = SceneId::new("second").unwrap();
	let data = document("before");
	let mut project =
		ProjectData::for_scene_with_id("Test", active.clone(), "scenes/main.json", &data).unwrap();
	project
		.scenes
		.insert(SceneDescriptor::new(second.clone(), "scenes/second.json").unwrap())
		.unwrap();
	let mut session = EditorSession::for_project(data, project).unwrap();
	session
		.load_saved_scenes(vec![(second.clone(), document("inactive"))])
		.unwrap();
	session
		.execute_command(EditorCommandRequest {
			expected_revision: session.revision(),
			command: Command::Rename {
				key: "actor".into(),
				name: "after".into(),
			},
		})
		.unwrap();
	let before = session.authored_document();
	let revision = session.revision();
	let scene_revision = session.scene_revision();
	assert!(
		session
			.history
			.preview_restore(&before, session.project(), false)
			.unwrap()
			.is_some()
	);
	// Simulate a workspace invariant rejected after the active patch was prepared.
	Arc::make_mut(&mut session.drafts.get_mut(&second).unwrap().data)
		.camera
		.near_plane = -1.0;
	for _ in 0..2 {
		assert!(session.restore_history(false).is_err());
		assert!(Arc::ptr_eq(&session.data, &before));
		assert_eq!(session.revision(), revision);
		assert_eq!(session.scene_revision(), scene_revision);
		assert!(session.history.can_undo());
		assert!(!session.history.can_redo());
	}
	let draft = session.drafts.get_mut(&second).unwrap();
	draft.data = Arc::clone(&draft.saved);
	session.restore_history(false).unwrap();
	assert_eq!(session.data.entities[0].name, "before");
	assert!(!session.history.can_undo());
	assert!(session.history.can_redo());
}
