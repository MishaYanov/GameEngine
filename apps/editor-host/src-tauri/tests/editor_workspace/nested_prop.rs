use super::*;
use bridge::viewport::PropPreviewTargetDto;
use editor::prefab_sources::{nested::NestedPropPlacementDto, PropDraftEdit};
use runtime::linked_prefab::PlacementId;
use runtime::scene_data::TransformData;

#[test]
fn reviewed_child_is_reused_with_independent_transforms_and_shared_draft_history() {
	let fixture = Fixture::new();
	let mut workspace = fixture.open();
	run(
		&mut workspace,
		Command::Select {
			keys: vec!["a".into()],
		},
	);
	let revision = workspace.backend.session().revision();
	let base = workspace
		.create_linked_prefab_source("prefabs/base.json", Default::default(), revision)
		.unwrap();
	let child = workspace
		.create_linked_prefab_source("prefabs/child.json", Default::default(), revision)
		.unwrap();
	let review = workspace
		.nested_prop_source(&child.asset_id, &child.fingerprint)
		.unwrap();
	assert!(review.slots.is_empty());
	assert!(workspace
		.nested_prop_source(&child.asset_id, &"0".repeat(64))
		.is_err());
	let make_placement = |name: &str, x| NestedPropPlacementDto {
		asset_id: child.asset_id.clone(),
		fingerprint: child.fingerprint.clone(),
		name: name.into(),
		parent: EntityKey::new("a").unwrap(),
		local: TransformData {
			position: [x, 0.0, 0.0],
			..TransformData::default()
		},
		bindings: Default::default(),
	};
	let first = workspace
		.add_prop_draft_placement(
			&base.draft_id,
			base.draft_revision,
			make_placement("Left Grate", 1.0),
		)
		.unwrap();
	let second = workspace
		.add_prop_draft_placement(
			&base.draft_id,
			first.draft_revision,
			make_placement("Right Grate", 4.0),
		)
		.unwrap();
	assert!(second.dirty);
	assert_eq!(second.prefab["schema_version"], 4);
	assert_eq!(
		second.prefab["composition"]["placements"]
			.as_array()
			.unwrap()
			.len(),
		2
	);
	assert_eq!(
		second.prefab["composition"]["pins"]
			.as_array()
			.unwrap()
			.len(),
		1
	);
	let preview = workspace
		.open_prop_preview(
			PropPreviewTargetDto::Source {
				draft_id: base.draft_id.clone(),
			},
			revision,
			workspace.generation,
			1,
		)
		.unwrap();
	assert!(workspace.prop_preview_is_current(&preview.token));
	assert_eq!(
		workspace
			.active_prop_preview()
			.unwrap()
			.document
			.entities
			.len(),
		5
	);
	let undone = workspace
		.edit_prop_draft(&base.draft_id, second.draft_revision, PropDraftEdit::Undo)
		.unwrap();
	assert_eq!(
		workspace
			.active_prop_preview()
			.unwrap()
			.document
			.entities
			.len(),
		3
	);
	let redone = workspace
		.edit_prop_draft(&base.draft_id, undone.draft_revision, PropDraftEdit::Redo)
		.unwrap();
	let saved = workspace
		.save_prop_draft(&base.draft_id, redone.draft_revision)
		.unwrap();
	assert!(!saved.draft.dirty);
	assert_eq!(workspace.backend.session().revision(), revision);
}

#[test]
fn refreshing_one_placement_leaves_its_sibling_pinned_and_respects_draft_ownership() {
	let fixture = Fixture::new();
	let mut workspace = fixture.open();
	run(
		&mut workspace,
		Command::Select {
			keys: vec!["a".into()],
		},
	);
	let revision = workspace.backend.session().revision();
	let outer = workspace
		.create_linked_prefab_source("prefabs/outer.json", Default::default(), revision)
		.unwrap();
	let inner = workspace
		.create_linked_prefab_source("prefabs/inner.json", Default::default(), revision)
		.unwrap();
	let request = |name: &str| NestedPropPlacementDto {
		asset_id: inner.asset_id.clone(),
		fingerprint: inner.fingerprint.clone(),
		name: name.into(),
		parent: EntityKey::new("a").unwrap(),
		local: TransformData::default(),
		bindings: Default::default(),
	};
	let first = workspace
		.add_prop_draft_placement(&outer.draft_id, outer.draft_revision, request("One"))
		.unwrap();
	let second = workspace
		.add_prop_draft_placement(&outer.draft_id, first.draft_revision, request("Two"))
		.unwrap();
	let id = PlacementId::new(
		second.prefab["composition"]["placements"][0]["id"]
			.as_str()
			.unwrap(),
	)
	.unwrap();
	let mut new_prefab = inner.prefab.clone();
	new_prefab["entities"][0]["name"] = "Updated Inner".into();
	let updated = workspace
		.edit_prop_draft(
			&inner.draft_id,
			inner.draft_revision,
			PropDraftEdit::Apply { prefab: new_prefab },
		)
		.unwrap();
	let published = workspace
		.save_prop_draft(&inner.draft_id, updated.draft_revision)
		.unwrap();
	assert_ne!(published.draft.fingerprint, inner.fingerprint);
	assert!(workspace
		.refresh_prop_draft_placement(
			&outer.draft_id,
			first.draft_revision,
			&id,
			&published.draft.fingerprint,
		)
		.is_err());
	assert!(workspace
		.refresh_prop_draft_placement(
			&outer.draft_id,
			second.draft_revision,
			&id,
			&inner.fingerprint,
		)
		.is_err());
	let refreshed = workspace
		.refresh_prop_draft_placement(
			&outer.draft_id,
			second.draft_revision,
			&id,
			&published.draft.fingerprint,
		)
		.unwrap();
	let placements = refreshed.prefab["composition"]["placements"]
		.as_array()
		.unwrap();
	assert_eq!(
		placements[0]["target"]["fingerprint"],
		published.draft.fingerprint
	);
	assert_eq!(placements[1]["target"]["fingerprint"], inner.fingerprint);
	assert_eq!(
		refreshed.prefab["composition"]["pins"]
			.as_array()
			.unwrap()
			.len(),
		2
	);
	assert_eq!(workspace.backend.session().revision(), revision);
}
