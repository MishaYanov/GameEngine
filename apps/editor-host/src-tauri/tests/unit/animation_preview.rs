use super::*;
use crate::project_location::ProjectLocation;
use std::{fs, path::PathBuf};
struct Fixture(PathBuf);
impl Fixture {
	fn new() -> Self {
		let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
		let root = repository.join("target").join(format!(
			"qa-animation-preview-{}",
			AssetId::generate().as_str()
		));
		for directory in ["assets/scenes", "assets/characters", "assets/props"] {
			fs::create_dir_all(root.join(directory)).unwrap();
		}
		fs::write(
			root.join("project.json"),
			test_project::manifest("animation-room"),
		)
		.unwrap();
		for file in [
			"asset-catalog.json",
			"scenes/animation-room.json",
			"characters/two-joint.glb",
			"props/cube.glb",
			"props/capsule.glb",
		] {
			fs::copy(
				repository.join("apps/game/assets").join(file),
				root.join("assets").join(file),
			)
			.unwrap();
		}
		Self(root)
	}
	fn workspace(&self) -> EditorWorkspace {
		EditorWorkspace::open_project_with_notifier(
			ProjectLocation::from_manifest(&self.0.join("project.json")).unwrap(),
			|| {},
		)
		.unwrap()
	}
}
impl Drop for Fixture {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}
#[test]
fn scrub_is_native_isolated_pinned_and_rejects_invalid_time_without_losing_pose() {
	let fixture = Fixture::new();
	let mut workspace = fixture.workspace();
	let before = workspace.backend.session().authored_document();
	let revision = workspace.backend.session().revision();
	let preview = workspace
		.open_animation_preview(EntityKey::new("player").unwrap(), 1, revision)
		.unwrap();
	let first = workspace.active_animation_preview().unwrap().1;
	workspace
		.sample_animation_preview(&preview.token, 0.5)
		.unwrap();
	let sampled = workspace.active_animation_preview().unwrap().1;
	assert_ne!(first.local, sampled.local);
	assert!(workspace
		.sample_animation_preview(&preview.token, f64::NAN)
		.is_err());
	assert!(Arc::ptr_eq(
		&sampled,
		&workspace.active_animation_preview().unwrap().1
	));
	assert_eq!(workspace.backend.session().authored_document(), before);
	assert_eq!(workspace.backend.session().revision(), revision);
	assert!(!workspace.backend.session().is_playing());
	fs::write(
		fixture.0.join("assets/characters/two-joint.glb"),
		b"changed source",
	)
	.unwrap();
	workspace
		.sample_animation_preview(&preview.token, 0.25)
		.unwrap(); // immutable accepted source remains pinned
	assert!(workspace
		.open_animation_preview(EntityKey::new("player").unwrap(), 1, revision)
		.is_err());
	workspace.close_animation_preview(&preview.token);
	assert!(workspace.active_animation_preview().is_none());
}
#[test]
fn revision_generation_and_replacement_tokens_invalidate_preview() {
	let fixture = Fixture::new();
	let mut workspace = fixture.workspace();
	let revision = workspace.backend.session().revision();
	let old = workspace
		.open_animation_preview(EntityKey::new("player").unwrap(), 0, revision)
		.unwrap();
	let current = workspace
		.open_animation_preview(EntityKey::new("player").unwrap(), 1, revision)
		.unwrap();
	workspace.close_animation_preview(&old.token);
	assert!(workspace.active_animation_preview().is_some());
	assert!(workspace.sample_animation_preview(&old.token, 0.).is_err());
	workspace.backend.supersede_revision(revision).unwrap();
	assert!(workspace.active_animation_preview().is_none());
	assert!(workspace
		.sample_animation_preview(&current.token, 0.)
		.is_err());
	let next = workspace
		.open_animation_preview(
			EntityKey::new("player").unwrap(),
			0,
			workspace.backend.session().revision(),
		)
		.unwrap();
	workspace.generation += 1;
	assert!(workspace.active_animation_preview().is_none());
	assert!(workspace.sample_animation_preview(&next.token, 0.).is_err());
}

#[test]
fn workspace_scopes_nested_instances_and_source_drafts_to_their_own_preview_tokens() {
	use crate::animation_preview::scoped::AnimationPreviewDocument;
	use crate::editor::prefab_sources::PropDraftEdit;
	use bridge::editor::{EditorCommandDto, EditorCommandRequest};
	use runtime::{
		components::NO_MESH_PATH,
		editor::PrefabData,
		linked_prefab::{
			LinkedPrefabSource, NestedPlacement, PinnedAssetVersion, PlacementId, PropAddress,
		},
		scene_data::{EntityData, TransformData},
	};

	let fixture = Fixture::new();
	let mut workspace = fixture.workspace();
	let scene = workspace.backend.session().authored_document();
	let actor = scene
		.entities
		.iter()
		.find(|item| item.key.as_str() == "player")
		.unwrap()
		.clone();
	let component_id = actor.animation.as_ref().unwrap().id.clone();
	let actor_key = actor.key.clone();
	let child = LinkedPrefabSource {
		source_id: AssetId::generate(),
		path: "character-prop.json".to_owned(),
		fingerprint: "a".repeat(64),
		prefab: PrefabData {
			schema_version: 3,
			entities: vec![actor],
			gameplay: None,
			reference_slots: Default::default(),
			composition: Default::default(),
		},
	};

	let parent = EntityKey::new("stage").unwrap();
	let stage = PrefabData {
		schema_version: 3,
		entities: vec![
			EntityData::new(parent.clone(), "Stage", NO_MESH_PATH).with_renderable(false)
		],
		gameplay: None,
		reference_slots: Default::default(),
		composition: Default::default(),
	};
	let registered = runtime::assets::project_assets::prepare_prefab(
		&workspace.asset_root,
		"stage.json",
		&stage,
	)
	.unwrap()
	.publish()
	.unwrap();
	let placement_id = PlacementId::new("performer").unwrap();
	let assembly = stage
		.with_nested_placement(
			&registered.id,
			&child,
			NestedPlacement {
				id: placement_id.clone(),
				name: "Performer".to_owned(),
				parent,
				local: TransformData::default(),
				target: PinnedAssetVersion {
					asset_id: child.source_id.clone(),
					fingerprint: child.fingerprint.clone(),
				},
				bindings: Default::default(),
			},
		)
		.unwrap();
	let registered = runtime::assets::project_assets::prepare_prefab(
		&workspace.asset_root,
		"stage.json",
		&assembly,
	)
	.unwrap()
	.publish()
	.unwrap();
	let source = LinkedPrefabSource {
		source_id: registered.id.clone(),
		path: registered.path.clone(),
		fingerprint: registered.fingerprint,
		prefab: assembly,
	};
	for x in [4.0, 12.0] {
		workspace
			.backend
			.execute(EditorCommandRequest {
				expected_revision: workspace.backend.session().revision(),
				command: EditorCommandDto::InstantiateLinkedPrefab {
					source: source.clone(),
					position: [x, 0.0, 0.0],
					bindings: Default::default(),
				},
			})
			.unwrap();
	}
	let revision = workspace.backend.session().revision();
	let scene_id = workspace.backend.session().active_scene_id().clone();
	let generation = workspace.generation;
	let before = workspace.backend.session().authored_document();
	let bytes = before.to_json().unwrap();
	let address = PropAddress::Object {
		placement_path: vec![placement_id],
		source_entity: actor_key.clone(),
	};
	let expanded_key = address.materialized_key().unwrap();
	let instances: Vec<_> = before
		.entities
		.iter()
		.filter_map(|entity| {
			entity
				.linked_prefab
				.as_ref()
				.map(|pin| AnimationWorkspaceTarget {
					document: AnimationPreviewDocument::Instance {
						scene_id: scene_id.clone(),
						root_key: entity.key.clone(),
					},
					entity_key: pin.entity_map[&expanded_key].clone(),
					component_id: component_id.clone(),
					address: Some(address.clone()),
				})
		})
		.collect();
	assert_eq!(instances.len(), 2);
	assert_ne!(instances[0].entity_key, instances[1].entity_key);
	let first = workspace
		.open_animation_workspace_preview(instances[0].clone(), 1, revision, generation, 1)
		.unwrap();
	assert_eq!(first.target, Some(instances[0].clone()));
	assert!(workspace.active_animation_preview().is_none());
	let first_pose = workspace.active_prop_preview().unwrap().pose.unwrap().1;
	workspace
		.sample_animation_preview(&first.token, 0.5)
		.unwrap();
	assert_ne!(
		workspace
			.active_prop_preview()
			.unwrap()
			.pose
			.unwrap()
			.1
			.local,
		first_pose.local
	);

	let second = workspace
		.open_animation_workspace_preview(instances[1].clone(), 0, revision, generation, 2)
		.unwrap();
	assert_eq!(second.target, Some(instances[1].clone()));
	assert!(workspace
		.sample_animation_preview(&first.token, 0.0)
		.is_err());
	workspace.close_animation_preview(&first.token);
	assert!(workspace
		.open_animation_workspace_preview(instances[0].clone(), 0, revision, generation, 1)
		.is_err());
	assert!(workspace.animation_aperture_is_current(&second.token));
	let mut wrong_instance = instances[1].clone();
	wrong_instance.entity_key = instances[0].entity_key.clone();
	assert!(workspace
		.open_animation_workspace_preview(wrong_instance, 0, revision, generation, 3)
		.is_err());

	let draft = workspace
		.open_prefab_source("stage.json", revision)
		.unwrap();
	let scoped_source = AnimationWorkspaceTarget {
		document: AnimationPreviewDocument::Source {
			asset_id: registered.id,
			draft_id: draft.draft_id.clone(),
			draft_revision: draft.draft_revision,
		},
		entity_key: actor_key,
		component_id,
		address: Some(address),
	};
	let source_preview = workspace
		.open_animation_workspace_preview(scoped_source.clone(), 0, revision, generation, 4)
		.unwrap();
	assert_eq!(source_preview.target, Some(scoped_source));
	assert_eq!(source_preview.entity_key, expanded_key);
	assert!(workspace
		.sample_animation_preview(&second.token, 0.0)
		.is_err());
	workspace.frame_prop_preview(&source_preview.token).unwrap();
	assert_eq!(workspace.active_prop_preview().unwrap().frame_request, 1);

	let mut edited = draft.prefab;
	edited["entities"][0]["name"] = serde_json::json!("Edited stage");
	workspace
		.edit_prop_draft(
			&draft.draft_id,
			draft.draft_revision,
			PropDraftEdit::Apply { prefab: edited },
		)
		.unwrap();
	assert!(workspace
		.sample_animation_preview(&source_preview.token, 0.0)
		.is_err());
	assert!(workspace.discard_stale_animation_preview());
	assert!(workspace.active_prop_preview().is_none());
	assert_eq!(workspace.backend.session().revision(), revision);
	assert_eq!(
		workspace
			.backend
			.session()
			.authored_document()
			.to_json()
			.unwrap(),
		bytes
	);
}

#[path = "../../../../../tests/support/project_fixture.rs"]
mod test_project;
