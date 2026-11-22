//! Local document history for an open Prop source.
//!
//! Callers admit only validated, canonical prefab documents. This history has its
//! own revision and byte budget and never changes the scene command journal.

use std::{collections::VecDeque, sync::Arc};

const MAX_DOCUMENTS: usize = 33;
const MAX_HISTORY_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone)]
pub(super) struct DraftHistory {
	documents: VecDeque<Arc<str>>,
	cursor: usize,
	bytes: usize,
	revision: u64,
}

impl DraftHistory {
	pub(super) fn new(document: Arc<str>) -> Self {
		Self {
			bytes: document.len(),
			documents: VecDeque::from([document]),
			cursor: 0,
			revision: 0,
		}
	}

	pub(super) fn revision(&self) -> u64 {
		self.revision
	}

	pub(super) fn document(&self) -> &Arc<str> {
		&self.documents[self.cursor]
	}

	pub(super) fn can_undo(&self) -> bool {
		self.cursor > 0
	}

	pub(super) fn can_redo(&self) -> bool {
		self.cursor + 1 < self.documents.len()
	}

	pub(super) fn apply(&mut self, expected: u64, document: Arc<str>) -> Result<(), String> {
		self.check(expected)?;
		if self.document().as_ref() == document.as_ref() {
			return Ok(());
		}
		if document.len() > MAX_HISTORY_BYTES {
			return Err("The Prop draft exceeds its history budget.".into());
		}
		let revision = self.next_revision()?;
		for discarded in self.documents.drain(self.cursor + 1..) {
			self.bytes -= discarded.len();
		}
		self.bytes += document.len();
		self.documents.push_back(document);
		while self.documents.len() > MAX_DOCUMENTS || self.bytes > MAX_HISTORY_BYTES {
			if let Some(discarded) = self.documents.pop_front() {
				self.bytes -= discarded.len();
			}
		}
		self.cursor = self.documents.len() - 1;
		self.revision = revision;
		Ok(())
	}

	pub(super) fn undo(&mut self, expected: u64) -> Result<(), String> {
		self.check(expected)?;
		if self.can_undo() {
			self.revision = self.next_revision()?;
			self.cursor -= 1;
		}
		Ok(())
	}

	pub(super) fn redo(&mut self, expected: u64) -> Result<(), String> {
		self.check(expected)?;
		if self.can_redo() {
			self.revision = self.next_revision()?;
			self.cursor += 1;
		}
		Ok(())
	}

	pub(super) fn discard(&mut self, expected: u64, saved: Arc<str>) -> Result<(), String> {
		self.check(expected)?;
		if self.documents.len() == 1 && self.document().as_ref() == saved.as_ref() {
			return Ok(());
		}
		let revision = self.next_revision()?;
		*self = Self::new(saved);
		self.revision = revision;
		Ok(())
	}

	pub(super) fn check(&self, expected: u64) -> Result<(), String> {
		if expected != self.revision {
			return Err("The Prop draft changed. Review its current values and try again.".into());
		}
		Ok(())
	}

	fn next_revision(&self) -> Result<u64, String> {
		self.revision.checked_add(1).ok_or_else(|| {
			"The Prop draft revision is exhausted. Close and reopen the Prop.".into()
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn stale_and_noop_edits_preserve_redo() {
		let mut history = DraftHistory::new("original".into());
		history.apply(0, "first change".into()).unwrap();
		history.undo(1).unwrap();
		assert!(history.apply(1, "stale".into()).is_err());
		history.apply(2, "original".into()).unwrap();
		assert_eq!(history.revision(), 2);
		history.redo(2).unwrap();
		assert_eq!(history.document().as_ref(), "first change");
		history.undo(3).unwrap();
		history.apply(4, "replacement".into()).unwrap();
		assert!(!history.can_redo());
		assert_eq!(history.document().as_ref(), "replacement");
	}

	#[test]
	fn history_bounds_the_number_of_versions_and_keeps_the_newest_document() {
		let mut history = DraftHistory::new("start".into());
		for step in 0..MAX_DOCUMENTS + 3 {
			history
				.apply(history.revision(), format!("save {step}").into())
				.unwrap();
		}
		assert_eq!(history.documents.len(), MAX_DOCUMENTS);
		assert_eq!(
			history.document().as_ref(),
			format!("save {}", MAX_DOCUMENTS + 2)
		);
	}
}
