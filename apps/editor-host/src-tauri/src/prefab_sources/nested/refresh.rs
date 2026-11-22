use super::*;

impl EditorWorkspace {
	/// Refresh exactly one reviewed placement in the owning draft.
	pub fn refresh_prop_draft_placement(
		&mut self,
		id: &str,
		expected_revision: u64,
		placement_id: &PlacementId,
		fingerprint: &str,
	) -> Result<PrefabSourceDraftDto, String> {
		let mut next = self.editable_prop_draft(id, expected_revision)?.clone();
		let document = PrefabData::from_json(next.history.document())?;
		let placement = document
			.composition
			.placements
			.iter()
			.find(|placement| &placement.id == placement_id)
			.ok_or("The selected nested placement no longer belongs to this Prop")?;
		let source = self.checked_nested_prop(&placement.target.asset_id, fingerprint)?;
		let candidate =
			document.with_refreshed_nested_placement(&next.source_id, placement_id, &source)?;
		next.history
			.apply(expected_revision, candidate.to_json()?.into())?;
		self.install_prop_draft(id, next)
	}
}
