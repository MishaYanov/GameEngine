//! Versioned and redacted transfer receipts. Viewing a portable file is read-only.

use super::{CHECKPOINT_ID, PROJECT_ID, SLOT_NUMBER, sha256, valid_identifier};
use crate::local_player_files::read_bounded;
use serde::{Deserialize, Serialize};
use std::{
	path::Path,
	time::{SystemTime, UNIX_EPOCH},
};

pub(super) const MAX_AUDIT_BYTES: usize = 32 * 1024;
pub(super) const MAX_ROWS: usize = 64;
pub(crate) const AUDIT_SUFFIX: &str = ".checkpoint-audit.json";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Direction {
	Import,
	Export,
}

impl Direction {
	pub(crate) fn label(self) -> &'static str {
		match self {
			Self::Import => "Import",
			Self::Export => "Export",
		}
	}
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Receipt {
	pub(crate) slot: u8,
	pub(crate) direction: Direction,
	pub(crate) checkpoint_id: String,
	pub(crate) completed_unix_seconds: u64,
}

impl Receipt {
	pub(super) fn cinder(direction: Direction) -> Self {
		Self {
			slot: SLOT_NUMBER,
			direction,
			checkpoint_id: CHECKPOINT_ID.into(),
			completed_unix_seconds: SystemTime::now()
				.duration_since(UNIX_EPOCH)
				.map_or(0, |elapsed| elapsed.as_secs()),
		}
	}

	fn is_valid(&self) -> bool {
		(1..=3).contains(&self.slot) && valid_identifier(&self.checkpoint_id)
	}

	pub(super) fn is_cinder(&self) -> bool {
		self.slot == SLOT_NUMBER && self.checkpoint_id == CHECKPOINT_ID
	}
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AuditScope {
	pub(crate) project_id: String,
	pub(crate) slot: Option<u8>,
	pub(crate) direction: Option<Direction>,
}

impl AuditScope {
	fn cinder(direction: Option<Direction>) -> Self {
		Self {
			project_id: PROJECT_ID.into(),
			slot: Some(SLOT_NUMBER),
			direction,
		}
	}

	fn covers(&self, receipt: &Receipt) -> bool {
		receipt.is_cinder()
			&& self.slot.is_none_or(|slot| receipt.slot == slot)
			&& self
			.direction
			.is_none_or(|direction| receipt.direction == direction)
	}
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyAudit {
	schema_version: u32,
	entries: Vec<Receipt>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ScopedAudit {
	schema_version: u32,
	scope: AuditScope,
	entries: Vec<Receipt>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum WireAudit {
	Scoped(ScopedAudit),
	Legacy(LegacyAudit),
}

#[derive(Debug)]
pub(crate) struct AuditReview {
	pub(crate) scope: Option<AuditScope>,
	pub(crate) entries: Vec<Receipt>,
	pub(crate) first_completed: Option<u64>,
	pub(crate) last_completed: Option<u64>,
	pub(crate) source_sha256: [u8; 32],
}

impl AuditReview {
	pub(crate) fn read(path: &Path) -> Result<Self, String> {
		let bytes = read_bounded(path, MAX_AUDIT_BYTES)?.ok_or("Audit file is unavailable")?;
		Self::decode(&bytes)
	}

	fn decode(bytes: &[u8]) -> Result<Self, String> {
		let (scope, entries) =
			match serde_json::from_slice(bytes).map_err(|_| "Invalid checkpoint audit")? {
				WireAudit::Legacy(audit) if audit.schema_version == 1 => (None, audit.entries),
				WireAudit::Scoped(audit)
				if audit.schema_version == 2
					&& audit.scope.project_id == PROJECT_ID
					&& audit.scope.slot.is_none_or(|slot| slot == SLOT_NUMBER)
					&& audit.entries.iter().all(|row| audit.scope.covers(row)) =>
					{
						(Some(audit.scope), audit.entries)
					}
				_ => return Err("Unsupported checkpoint audit or inconsistent scope".into()),
			};
		if entries.len() > MAX_ROWS || !entries.iter().all(Receipt::is_valid) {
			return Err("Invalid or too many audit entries".into());
		}
		let timestamps = || entries.iter().map(|row| row.completed_unix_seconds);
		let first_completed = timestamps().min();
		let last_completed = timestamps().max();
		Ok(Self {
			scope,
			entries,
			first_completed,
			last_completed,
			source_sha256: sha256(bytes),
		})
	}
}

pub(super) fn export_bytes(
	rows: &[Receipt],
	direction: Option<Direction>,
) -> Result<Vec<u8>, String> {
	let scope = AuditScope::cinder(direction);
	let entries = rows
		.iter()
		.filter(|row| scope.covers(row))
		.cloned()
		.collect();
	serde_json::to_vec_pretty(&ScopedAudit {
		schema_version: 2,
		scope,
		entries,
	})
		.map_err(|error| error.to_string())
}
