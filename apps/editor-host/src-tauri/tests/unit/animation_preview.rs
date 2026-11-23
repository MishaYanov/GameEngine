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
	let key_page = workspace
		.animation_preview_keys(&preview.token, 0, 0)
		.unwrap();
	let key_page = serde_json::to_value(key_page).unwrap();
	assert_eq!(key_page["clip_index"], 1);
	assert!(!key_page["keys"].as_array().unwrap().is_empty());
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

#[test]
fn visual_clip_save_reopen_and_preview_keep_scene_and_invalid_assets_separate() {
	use runtime::{
		animation::{
			AuthoredClip, AuthoredClipOwner, ClipKey, ScopedTrack, VisualInterpolation,
			VisualTargetId, VisualTrack,
		},
		components::NO_MESH_PATH,
		editor::PrefabData,
		linked_prefab::PropAddress,
		scene_data::{ColliderData, EntityData},
	};

	let fixture = Fixture::new();
	let mut workspace = fixture.workspace();
	let pivot = EntityKey::new("pivot").unwrap();
	let lever = EntityKey::new("lever").unwrap();
	let prop_path = "lever.json";
	let mut prefab = PrefabData {
		schema_version: 3,
		entities: vec![
			EntityData::new(pivot.clone(), "Pivot", NO_MESH_PATH).with_renderable(false),
			EntityData::new(lever.clone(), "Lever", "props/cube.glb").with_parent(Some(pivot)),
		],
		gameplay: None,
		reference_slots: Default::default(),
		composition: Default::default(),
	};
	let prop =
		runtime::assets::project_assets::prepare_prefab(&workspace.asset_root, prop_path, &prefab)
			.unwrap()
			.publish()
			.unwrap();
	let target = VisualTargetId::Prop {
		address: PropAddress::Object {
			placement_path: vec![],
			source_entity: lever.clone(),
		},
	};
	let clip = AuthoredClip {
		schema_version: 1,
		name: "Lever Turn".into(),
		owner: AuthoredClipOwner::Prop {
			asset_id: prop.id.clone(),
		},
		duration_ticks: 1_000,
		markers: Vec::new(),
		tracks: vec![
			ScopedTrack {
				target: target.clone(),
				track: VisualTrack::LocalPosition {
					interpolation: VisualInterpolation::Linear,
					keys: vec![
						ClipKey {
							id: None,
							at: 0,
							value: [0.0; 3],
						},
						ClipKey {
							id: None,
							at: 1000,
							value: [2.0, 0.0, 0.0],
						},
					],
				},
			},
			ScopedTrack {
				target,
				track: VisualTrack::LocalRotation {
					interpolation: VisualInterpolation::Linear,
					keys: vec![
						ClipKey {
							id: None,
							at: 0,
							value: [0.0, 0.0, 0.0, 1.0],
						},
						ClipKey {
							id: None,
							at: 1000,
							value: [0.0, 1.0, 0.0, 0.0],
						},
					],
				},
			},
		],
	};
	let before = workspace.backend.session().authored_document();
	let revision = workspace.backend.session().revision();
	let generation = workspace.generation;
	let saved = workspace
		.save_authored_clip_asset("lever-turn.clip.json", clip.clone(), None, revision)
		.unwrap()
		.document;
	assert_eq!(workspace.authored_clip_assets().unwrap().len(), 1);
	let reopened = workspace
		.open_authored_clip_asset(&saved.asset.asset_id)
		.unwrap();
	assert_eq!(reopened.clip, clip);
	assert_eq!(reopened.asset.fingerprint, saved.asset.fingerprint);
	let revision = workspace.backend.session().revision();
	let preview = workspace
		.open_authored_clip_preview(
			&saved.asset.asset_id,
			&saved.asset.fingerprint,
			None,
			revision,
			generation,
			1,
		)
		.unwrap();
	let half = workspace
		.sample_authored_clip_preview(&preview.token, 500)
		.unwrap();
	assert_eq!(half.poses[0].position, [1.0, 0.0, 0.0]);
	assert_eq!(half.poses[0].entity_key, lever);
	assert!(workspace
		.active_prop_preview()
		.unwrap()
		.visual_pose
		.is_some());
	assert_eq!(workspace.backend.session().authored_document(), before);
	assert_eq!(workspace.backend.session().revision(), revision);
	assert!(workspace
		.sample_authored_clip_preview(&preview.token, 1001)
		.is_err());
	assert!(workspace.close_authored_clip_preview(&preview.token));
	assert!(workspace.active_prop_preview().is_none());

	use crate::animation_preview::timeline::{ClipDraftOperation, PreviewKeyMove};
	use runtime::animation::TimelineEdit;
	let file_path = workspace.asset_root.join("lever-turn.clip.json");
	let old_bytes = fs::read(&file_path).unwrap();
	let cursor = workspace.backend.change_cursor();
	let timeline = workspace
		.open_clip_timeline_draft(&saved.asset.asset_id, revision)
		.unwrap();
	assert_eq!(timeline.clip.schema_version, 2);
	assert_eq!(fs::read(&file_path).unwrap(), old_bytes);
	assert_eq!(
		timeline.draft_id,
		workspace
			.open_clip_timeline_draft(&saved.asset.asset_id, revision)
			.unwrap()
			.draft_id
	);
	let key = match &timeline.clip.tracks[0].track {
		VisualTrack::LocalPosition { keys, .. } => keys[1].id.clone().unwrap(),
		_ => panic!("expected position"),
	};
	let preview = workspace
		.open_clip_timeline_preview(&timeline.draft_id, 0, None, revision, generation, 2)
		.unwrap();
	let dragged = workspace
		.sample_clip_timeline_preview(
			&preview.token,
			450,
			Some(PreviewKeyMove {
				key_id: key.clone(),
				at: 900,
			}),
		)
		.unwrap();
	assert_eq!(dragged.poses[0].position, [1.0, 0.0, 0.0]);
	let cancelled = workspace
		.sample_clip_timeline_preview(&preview.token, 450, None)
		.unwrap();
	assert!((cancelled.poses[0].position[0] - 0.9).abs() < 1.0e-6);
	assert!(
		!workspace
			.open_clip_timeline_draft(&saved.asset.asset_id, revision)
			.unwrap()
			.dirty
	);
	let moved = ClipDraftOperation::Edit {
		edit: TimelineEdit::MoveKey {
			key_id: key.clone(),
			at: 900,
		},
	};
	let draft = workspace
		.edit_clip_timeline_draft(&timeline.draft_id, 0, moved)
		.unwrap();
	assert_eq!(draft.draft_revision, 1);
	assert!(draft.can_undo && draft.dirty);
	assert!(workspace
		.sample_authored_clip_preview(&preview.token, 0)
		.is_err());
	assert!(workspace
		.edit_clip_timeline_draft(&timeline.draft_id, 0, ClipDraftOperation::Undo)
		.is_err());
	let draft = workspace
		.edit_clip_timeline_draft(&timeline.draft_id, 1, ClipDraftOperation::Undo)
		.unwrap();
	assert_eq!(draft.clip, timeline.clip);
	let draft = workspace
		.edit_clip_timeline_draft(&timeline.draft_id, 2, ClipDraftOperation::Redo)
		.unwrap();
	assert!(draft.dirty);
	let draft = workspace
		.edit_clip_timeline_draft(
			&timeline.draft_id,
			3,
			ClipDraftOperation::Edit {
				edit: TimelineEdit::AddMarker {
					at: 420,
					label: "Lever click".into(),
				},
			},
		)
		.unwrap();
	let marker = draft.clip.markers[0].id.clone();
	let draft = workspace
		.edit_clip_timeline_draft(
			&timeline.draft_id,
			4,
			ClipDraftOperation::Edit {
				edit: TimelineEdit::RenameMarker {
					marker_id: marker.clone(),
					label: "Door click".into(),
				},
			},
		)
		.unwrap();
	assert_eq!(draft.clip.markers[0].id, marker);
	assert!(workspace
		.close_clip_timeline_draft(&timeline.draft_id, false)
		.is_err());
	assert_eq!(workspace.backend.change_cursor(), cursor);
	assert_eq!(fs::read(&file_path).unwrap(), old_bytes);
	let committed = workspace
		.save_clip_timeline_draft(&timeline.draft_id, 5, revision)
		.unwrap();
	assert!(!committed.draft.dirty);
	workspace
		.close_clip_timeline_draft(&timeline.draft_id, false)
		.unwrap();
	let revision = workspace.backend.session().revision();
	let reopened = workspace
		.open_clip_timeline_draft(&saved.asset.asset_id, revision)
		.unwrap();
	assert_eq!(reopened.clip.markers[0].id, marker);
	assert_eq!(reopened.clip.markers[0].label, "Door click");
	assert_eq!(workspace.backend.session().authored_document(), before);

	let mut invalid = clip.clone();
	if let VisualTrack::LocalPosition { keys, .. } = &mut invalid.tracks[0].track {
		keys[1].at = 0;
	}
	assert!(workspace
		.save_authored_clip_asset("invalid.clip.json", invalid, None, revision)
		.is_err());
	assert!(!workspace.asset_root.join("invalid.clip.json").exists());
	prefab.entities[1].collider = Some(ColliderData::Sphere {
		radius: 1.0,
		offset: [0.0; 3],
		interaction: Default::default(),
	});
	runtime::assets::project_assets::prepare_prefab(&workspace.asset_root, prop_path, &prefab)
		.unwrap()
		.publish()
		.unwrap();
	assert!(workspace
		.save_authored_clip_asset("physical.clip.json", clip, None, revision)
		.is_err());
	assert!(!workspace.asset_root.join("physical.clip.json").exists());
	assert_eq!(workspace.authored_clip_assets().unwrap().len(), 1);
}

#[path = "../../../../../tests/support/project_fixture.rs"]
mod test_project;

#[test]
fn practice_yard_graphs_reopen_and_discarding_a_prop_clip_edit_preserves_play() {
	use crate::animation_preview::timeline::ClipDraftOperation;
	use bridge::editor::{EditorCommandDto, EditorCommandRequest};
	use runtime::animation::{TimelineEdit, VisualTrack};
	use std::collections::HashSet;

	let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
	let source = repo.join("apps/headless/tests/fixtures/authoring-controller");
	let fixture = Fixture(repo.join("target").join(format!(
		"qa-authoring-graph-{}",
		AssetId::generate().as_str()
	)));
	let assets = fixture.0.join("assets");
	for folder in ["animations", "controls", "models", "prefabs", "scenes"] {
		fs::create_dir_all(assets.join(folder)).unwrap();
		for entry in fs::read_dir(source.join("assets").join(folder)).unwrap() {
			let entry = entry.unwrap();
			assert!(entry.file_type().unwrap().is_file());
			fs::copy(entry.path(), assets.join(folder).join(entry.file_name())).unwrap();
		}
	}
	fs::copy(
		source.join("graph-project.json"),
		fixture.0.join("project.json"),
	)
	.unwrap();
	let catalog_file = assets.join("asset-catalog.json");
	fs::copy(source.join("assets/asset-catalog.json"), &catalog_file).unwrap();
	let catalog_bytes = fs::read(&catalog_file).unwrap();
	let clip_file = assets.join("animations/yard-swing.clip.json");
	let clip_bytes = fs::read(&clip_file).unwrap();
	let clip_id = AssetId::new("90000000-0000-4000-8000-000000000007").unwrap();

	let mut workspace = fixture.workspace();
	let revision = workspace.backend.session().revision();
	let timeline = workspace
		.open_clip_timeline_draft(&clip_id, revision)
		.unwrap();
	let key = match &timeline.clip.tracks[0].track {
		VisualTrack::LocalPosition { keys, .. } => keys[1].id.clone().unwrap(),
		_ => panic!("expected a Training Blade position track"),
	};
	let edited = workspace
		.edit_clip_timeline_draft(
			&timeline.draft_id,
			0,
			ClipDraftOperation::Edit {
				edit: TimelineEdit::MoveKey {
					key_id: key,
					at: 120,
				},
			},
		)
		.unwrap();
	assert!(edited.dirty);
	workspace
		.close_clip_timeline_draft(&timeline.draft_id, true)
		.unwrap();
	assert_eq!(fs::read(&clip_file).unwrap(), clip_bytes);
	assert_eq!(fs::read(&catalog_file).unwrap(), catalog_bytes);
	assert_eq!(workspace.backend.session().revision(), revision);
	drop(workspace);

	let mut workspace = fixture.workspace();
	assert_eq!(
		workspace.open_authored_clip_asset(&clip_id).unwrap().clip,
		timeline.clip
	);
	let execute = |workspace: &mut EditorWorkspace, command| {
		workspace
			.execute_receipt(EditorCommandRequest {
				expected_revision: workspace.backend.session().revision(),
				command,
			})
			.unwrap();
	};
	execute(&mut workspace, EditorCommandDto::StartPlay);
	let authored = workspace.backend.session().authored_document();
	workspace
		.backend
		.set_action_controls_from_codes(
			&HashSet::from(["KeyF".to_owned()]),
			Default::default(),
			Default::default(),
		)
		.unwrap();
	workspace.backend.tick(1.0 / 60.0).unwrap();
	let inspection = workspace.backend.runtime_inspection(0, 32).unwrap();
	assert_eq!(inspection.animators[0].state_id, "swing");
	assert_eq!(
		inspection.animators[0].transition.as_deref(),
		Some("idle_to_swing")
	);
	let route = inspection.control_graph.unwrap();
	assert_eq!(route.actor.key.as_str(), "yard-player");
	assert!(route.output.intent.attack.pressed);
	assert_eq!(route.output.presentation.get("aim"), Some(&true));
	assert_eq!(
		workspace
			.backend
			.session()
			.animations()
			.unwrap()
			.visual_matrices()
			.len(),
		1
	);

	let status = workspace.backend.play_status().unwrap();
	let (prepared, _) = workspace
		.backend
		.prepare_restart_play(&status.run_id, workspace.backend.session().revision())
		.unwrap();
	workspace.backend.commit_play_scene(prepared).unwrap();
	workspace.backend.tick(1.0 / 60.0).unwrap();
	let fresh = workspace.backend.runtime_inspection(0, 32).unwrap();
	assert_ne!(inspection.run_id, fresh.run_id);
	assert_eq!(fresh.animators[0].state_id, "idle");
	assert!(!fresh.control_graph.unwrap().output.intent.attack.pressed);
	execute(&mut workspace, EditorCommandDto::StopPlay);
	assert!(workspace.backend.runtime_inspection(0, 32).is_err());
	assert_eq!(workspace.backend.session().authored_document(), authored);
	assert_eq!(fs::read(&clip_file).unwrap(), clip_bytes);
}
