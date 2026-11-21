use super::{ChangeAccumulator, MAX_CHANGE_KEYS, WorldChange};
use crate::EntityKey;

fn key(index: usize) -> EntityKey {
	EntityKey::new(format!("entity-{index}")).unwrap()
}

fn assert_matches_merges(changes: Vec<WorldChange>) -> WorldChange {
	let mut expected = WorldChange::default();
	let mut accumulated = ChangeAccumulator::default();
	for change in changes {
		expected.merge(change.clone());
		accumulated.merge(change);
	}
	let actual = accumulated.finish();
	assert_eq!(actual, expected);
	actual
}

#[test]
fn accumulation_keeps_first_seen_order_and_separate_changed_and_removed_keys() {
	let actual = assert_matches_merges(vec![
		WorldChange {
			changed_entities: vec![key(2), key(1), key(2)],
			removed_entities: vec![key(1), key(1)],
			camera_changed: true,
			..Default::default()
		},
		WorldChange::default(),
		WorldChange {
			changed_entities: vec![key(1), key(3)],
			removed_entities: vec![key(3), key(1)],
			topology_changed: true,
			lights_changed: true,
			..Default::default()
		},
	]);
	assert_eq!(actual.changed_entities, vec![key(2), key(1), key(3)]);
	assert_eq!(actual.removed_entities, vec![key(1), key(3)]);
	assert!(actual.camera_changed && actual.topology_changed && actual.lights_changed);
}

#[test]
fn accumulation_uses_one_budget_and_accepts_duplicates_at_capacity() {
	let full = WorldChange {
		changed_entities: (0..MAX_CHANGE_KEYS / 2).map(key).collect(),
		removed_entities: (MAX_CHANGE_KEYS / 2..MAX_CHANGE_KEYS).map(key).collect(),
		..Default::default()
	};
	let duplicate = WorldChange {
		changed_entities: vec![key(0)],
		removed_entities: vec![key(MAX_CHANGE_KEYS - 1)],
		..Default::default()
	};
	assert!(!assert_matches_merges(vec![full.clone(), duplicate.clone()]).full_resync);
	let actual = assert_matches_merges(vec![
		full,
		duplicate,
		WorldChange {
			changed_entities: vec![key(MAX_CHANGE_KEYS)],
			..Default::default()
		},
		WorldChange {
			camera_changed: true,
			lights_changed: true,
			..Default::default()
		},
	]);
	assert!(actual.full_resync && actual.camera_changed && actual.lights_changed);
	assert!(actual.changed_entities.is_empty() && actual.removed_entities.is_empty());
}

#[test]
fn explicit_resync_discards_accumulated_keys_but_keeps_later_flags() {
	let actual = assert_matches_merges(vec![
		WorldChange {
			changed_entities: vec![key(0)],
			..Default::default()
		},
		WorldChange {
			full_resync: true,
			removed_entities: vec![key(0)],
			..Default::default()
		},
		WorldChange {
			changed_entities: vec![key(1)],
			topology_changed: true,
			..Default::default()
		},
	]);
	assert!(actual.full_resync && actual.topology_changed);
	assert!(actual.changed_entities.is_empty() && actual.removed_entities.is_empty());
}
