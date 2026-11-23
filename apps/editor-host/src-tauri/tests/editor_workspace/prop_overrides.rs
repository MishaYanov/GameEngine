use super::*;
use editor::prefab_sources::nested::NestedPropPlacementDto;
use runtime::{
	editor::command::{TransformDto, TransformEditDto, TransformSpace},
	linked_prefab::LinkedPrefabProperty,
	scene_data::TransformData,
};

#[test]
fn applying_a_reviewed_placement_override_writes_the_root_source_and_preserves_the_instance_pin() {
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
	let draft = workspace
		.add_prop_draft_placement(
			&outer.draft_id,
			outer.draft_revision,
			NestedPropPlacementDto {
				asset_id: inner.asset_id,
				fingerprint: inner.fingerprint,
				name: "Grate".into(),
				parent: EntityKey::new("a").unwrap(),
				local: TransformData::default(),
				bindings: Default::default(),
			},
		)
		.unwrap();
	let saved = workspace
		.save_prop_draft(&draft.draft_id, draft.draft_revision)
		.unwrap();
	let pinned = saved.draft.fingerprint.clone();
	let placed = workspace
		.instantiate_linked_source(
			"prefabs/outer.json",
			&draft.draft_id,
			[0.0; 3],
			Default::default(),
			revision,
		)
		.unwrap();
	let root = &placed.selection[0];
	let scene = workspace.backend.session().authored_document();
	let grate = scene
		.entities
		.iter()
		.find(|entity| entity.name == "Grate")
		.unwrap();
	run(
		&mut workspace,
		Command::SetTransforms {
			edits: vec![TransformEditDto {
				key: grate.key.as_str().into(),
				transform: TransformDto {
					position: [3.5, 0.0, 0.0],
					..TransformDto::default()
				},
			}],
			space: TransformSpace::Local,
			snap: false,
		},
	);
	let current = workspace.backend.session().revision();
	let reviewed = workspace
		.open_prefab_source("prefabs/outer.json", current)
		.unwrap();
	let status = workspace
		.linked_prefab_status(root, current)
		.unwrap()
		.unwrap();
	let change = status
		.overrides
		.iter()
		.find(|item| item.target.property == LinkedPrefabProperty::TransformPosition)
		.unwrap();
	assert!(change.can_apply_to_root_source);
	assert!(workspace
		.apply_linked_override_to_source(
			root,
			&reviewed.draft_id,
			&change.address,
			&change.target.property,
			current - 1,
		)
		.is_err());
	let applied = workspace
		.apply_linked_override_to_source(
			root,
			&reviewed.draft_id,
			&change.address,
			&change.target.property,
			current,
		)
		.unwrap();
	assert_eq!(
		applied.draft.prefab["composition"]["placements"][0]["local"]["position"],
		serde_json::json!([3.5, 0.0, 0.0]),
	);
	let status = workspace
		.linked_prefab_status(root, current)
		.unwrap()
		.unwrap();
	assert!(status.source_changed);
	assert_eq!(status.source_fingerprint, pinned);
	assert_eq!(workspace.backend.session().revision(), current);

	let review = workspace
		.review_linked_source_overrides(root, &applied.draft.draft_id, None, current)
		.unwrap();
	assert_eq!(review.len(), 1);
	assert_eq!(review[0].incoming.as_ref(), Some(&review[0].local));
	assert!(!review[0].conflict);
	workspace
		.update_linked_source_reviewed(root, &applied.draft.draft_id, None, Vec::new(), current)
		.unwrap();
	let updated = workspace
		.linked_prefab_status(root, workspace.backend.session().revision())
		.unwrap()
		.unwrap();
	assert!(!updated.source_changed);
	assert_eq!(updated.source_fingerprint, applied.draft.fingerprint);
}
