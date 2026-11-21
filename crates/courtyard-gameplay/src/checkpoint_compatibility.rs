//! Reviewed, directional content upgrades for the Courtyard release fixture.

use super::CheckpointSave;

struct UpgradeRule {
	project: &'static str,
	scene: &'static str,
	checkpoint: &'static str,
	save_version: u32,
	from_content: [u8; 32],
	to_content: [u8; 32],
}

// The target changes only the unbound courtyard-cp-entry decoration's label.
// The old and new content digests use the same V1 serialization contract.
const APPROVED_UPGRADES: [UpgradeRule; 1] = [UpgradeRule {
	project: "c70545a2-181c-42d0-906e-87fb32547914",
	scene: "courtyard",
	checkpoint: "after_gate",
	save_version: 1,
	from_content: [
		0x9e, 0x54, 0x43, 0x26, 0x34, 0x8a, 0xc2, 0x9e, 0x0e, 0xc2, 0x01, 0xde, 0x9e, 0x23, 0x79,
		0x67, 0xb1, 0x54, 0x48, 0xcd, 0x46, 0xd6, 0x49, 0x3a, 0x96, 0x1a, 0xc1, 0x5e, 0xb1, 0x22,
		0x60, 0x3e,
	],
	to_content: [
		0x59, 0x9c, 0x5a, 0xc3, 0x55, 0xf8, 0x3c, 0x7c, 0x94, 0x9b, 0x39, 0x01, 0xe2, 0x7b, 0x57,
		0xf9, 0x36, 0x7a, 0xe0, 0x3c, 0xa4, 0x7a, 0x76, 0x69, 0xa6, 0x01, 0x28, 0xed, 0x8f, 0x56,
		0x0e, 0x41,
	],
}];

pub(super) fn approves_upgrade(saved: &CheckpointSave, target: [u8; 32]) -> bool {
	APPROVED_UPGRADES.iter().any(|rule| {
		rule.project == saved.project_save_id
			&& rule.scene == saved.scene_id.as_str()
			&& rule.checkpoint == saved.checkpoint_id
			&& rule.save_version == saved.version
			&& rule.from_content == saved.content
			&& rule.to_content == target
	})
}
