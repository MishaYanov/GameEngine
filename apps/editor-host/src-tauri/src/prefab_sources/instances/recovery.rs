//! Relink unavailable sources only after reviewing a current Asset capture.

use super::*;
use runtime::linked_prefab::{review_recovery_source, LinkedPrefabRecoveryReview};

impl EditorWorkspace {
	pub fn review_linked_source_recovery(
		&self,
		root_key: &str,
		path: &str,
		draft_id: &str,
		revision: u64,
	) -> Result<LinkedPrefabRecoveryReview, String> {
		let (root, source) = self.reviewed_recovery_source(root_key, path, draft_id, revision)?;
		review_recovery_source(&self.backend.session().authored_document(), &root, &source)
	}

	pub fn recover_linked_source(
		&mut self,
		root_key: &str,
		path: &str,
		draft_id: &str,
		revision: u64,
	) -> Result<EditorSnapshotDto, String> {
		let (root, source) = self.reviewed_recovery_source(root_key, path, draft_id, revision)?;
		self.backend
			.execute(EditorCommandRequest {
				expected_revision: revision,
				command: EditorCommandDto::RelinkLinkedPrefab {
					root_key: root.as_str().into(),
					source,
				},
			})
			.map_err(|error| error.to_string())?;
		self.cache_bounds();
		self.backend.snapshot().map_err(|error| error.to_string())
	}

	fn reviewed_recovery_source(
		&self,
		root_key: &str,
		path: &str,
		draft_id: &str,
		revision: u64,
	) -> Result<(EntityKey, LinkedPrefabSource), String> {
		self.prefab_edit_context(revision)?;
		let root = EntityKey::new(root_key)?;
		let scene = self.backend.session().authored_document();
		let link = scene
			.entities
			.iter()
			.find(|entity| entity.key == root)
			.and_then(|entity| entity.linked_prefab.as_ref())
			.ok_or("Select the linked Prop instance to recover")?;
		if self.current_linked_source(&link.source).1.is_ok() {
			return Err("The original Prop source is available again. Review the linked source update instead.".into());
		}
		let source = self.reviewed_prefab_source(draft_id, path, revision)?;
		Ok((root, source))
	}
}
