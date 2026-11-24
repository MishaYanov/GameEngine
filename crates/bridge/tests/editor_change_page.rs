use engine_core::events::{BackendEventKind, ChangeJournal};

#[test]
fn editor_change_page_matches_the_frontend_fixture() {
	let mut journal = ChangeJournal::default();
	journal
		.publish(10, 4, BackendEventKind::ProjectChanged)
		.unwrap();
	journal
		.publish(11, 4, BackendEventKind::StateChanged)
		.unwrap();
	let fixture =
		include_str!("../../../apps/editor/src/app/core/editor/fixtures/editor-change-page.json");
	let expected: serde_json::Value = serde_json::from_str(fixture).unwrap();
	assert_eq!(
		serde_json::to_value(journal.since(0).unwrap()).unwrap(),
		expected
	);
}
