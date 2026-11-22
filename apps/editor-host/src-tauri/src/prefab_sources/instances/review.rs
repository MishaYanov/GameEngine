use super::*;
use runtime::linked_prefab::{
	self, LinkedPrefabOverrideResolution, LinkedPrefabOverrideReviewDto, LinkedPrefabProperty,
	PropAddress,
};
use std::collections::BTreeMap;

impl EditorWorkspace {
	pub fn review_linked_source_overrides(
		&self,
		key: &str,
		draft_id: &str,
		bindings: Option<BTreeMap<String, EntityKey>>,
		revision: u64,
	) -> Result<Vec<LinkedPrefabOverrideReviewDto>, String> {
		let (root, source) = self.resolve_reviewed_instance_source(key, draft_id, revision)?;
		let scene = self.backend.session().authored_document();
		linked_prefab::review_source_overrides(&scene, &root, &source, bindings.as_ref())
	}

	pub fn update_linked_source_reviewed(
		&mut self,
		key: &str,
		draft_id: &str,
		bindings: Option<BTreeMap<String, EntityKey>>,
		resolutions: Vec<LinkedPrefabOverrideResolution>,
		revision: u64,
	) -> Result<EditorSnapshotDto, String> {
		let (root, source) = self.resolve_reviewed_instance_source(key, draft_id, revision)?;
		self.backend
			.execute(EditorCommandRequest {
				expected_revision: revision,
				command: EditorCommandDto::UpdateLinkedPrefabReviewed {
					root_key: root.as_str().into(),
					source,
					bindings,
					resolutions,
				},
			})
			.map_err(|error| error.to_string())?;
		self.cache_bounds();
		self.backend.snapshot().map_err(|error| error.to_string())
	}

	pub fn apply_linked_override_to_source(
		&mut self,
		key: &str,
		draft_id: &str,
		address: &PropAddress,
		property: &LinkedPrefabProperty,
		revision: u64,
	) -> Result<PrefabSourceSaveDto, String> {
		let (root, source) = self.resolve_reviewed_instance_source(key, draft_id, revision)?;
		let mut next = self.source_draft(draft_id, revision)?;
		if next.history.document().as_ref() != next.saved.as_ref() {
			return Err(
				"Finish the existing Prop draft before applying an instance override.".into(),
			);
		}
		let scene = self.backend.session().authored_document();
		let reviewed = linked_prefab::override_status(&scene, &root)?
			.into_iter()
			.find(|item| &item.address == address && &item.target.property == property)
			.filter(|item| item.can_apply_to_root_source)
			.ok_or("Open the owning Prop source to apply this override")?;
		let document = source.prefab.with_applied_prop_override(
			&source.source_id,
			address,
			property,
			&reviewed.base,
			reviewed.value,
		)?;
		next.history
			.apply(next.history.revision(), document.to_json()?.into())?;
		self.publish_prop_draft(draft_id, next)
	}

	pub(super) fn resolve_reviewed_instance_source(
		&self,
		key: &str,
		draft_id: &str,
		revision: u64,
	) -> Result<(EntityKey, LinkedPrefabSource), String> {
		self.prefab_edit_context(revision)?;
		let root = self
			.linked_root(key)?
			.ok_or("Select a linked Prop instance")?;
		let scene = self.backend.session().authored_document();
		let link = scene
			.entities
			.iter()
			.find(|entity| entity.key == root)
			.and_then(|entity| entity.linked_prefab.as_ref())
			.ok_or("The linked Prop instance is no longer available")?;
		let catalog = runtime::assets::project_assets::load_catalog(&self.asset_root)?;
		let asset = catalog
			.by_id(&link.source.source_id)
			.ok_or("Linked source identity is unavailable")?;
		let source = self.reviewed_prefab_source(draft_id, &asset.path, revision)?;
		if source.source_id != link.source.source_id {
			return Err("The selected Prop source changed during review".into());
		}
		Ok((root, source))
	}
}
