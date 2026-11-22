use super::*;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

#[test]
fn transfer_file_list_is_bounded_sorted_and_filters_nonfiles() {
	static NEXT: AtomicU64 = AtomicU64::new(1);
	let root = std::env::temp_dir().join(format!(
		"engine-transfer-list-{}-{}",
		std::process::id(),
		NEXT.fetch_add(1, Ordering::Relaxed),
	));
	fs::create_dir(&root).unwrap();
	for index in (0..36).rev() {
		fs::write(root.join(format!("{index:02}{SUFFIX}")), []).unwrap();
	}
	fs::write(root.join("external.txt"), []).unwrap();
	fs::create_dir(root.join(format!("directory{SUFFIX}"))).unwrap();
	let files = files_in(&root).unwrap();
	assert_eq!(files.len(), 32);
	assert_eq!(files[0].label, "Transfer 01: 00");
	assert_eq!(files[31].label, "Transfer 32: 31");
	assert_eq!(files[0].path, root.join(format!("00{SUFFIX}")));
	assert_eq!(
		export_name(SaveSlot::Two, 123),
		format!("slot-2-123{SUFFIX}")
	);
	fs::remove_dir_all(root).unwrap();
}

#[test]
fn transfer_labels_keep_a_visible_identity_for_similar_names() {
	for (number, prefix) in [(1, 'á'), (2, 'â')] {
		let name = format!("{prefix}abcdefghijklmnopqrstuvwxyz{SUFFIX}");
		assert_eq!(
			transfer_label(number, &name),
			format!("Transfer {number:02}: ?abcdefghijklmnopqrstu...")
		);
	}
}
