//! Review an Assets entry and adopt its verified bytes into an open Prop draft.

use super::*;
use runtime::{
	linked_prefab::{NestedBinding, NestedPlacement, PinnedAssetVersion, PlacementId},
	scene_data::TransformData,
};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Serialize)]
pub struct NestedPropSourceDto {
	pub asset_id: AssetId,
	pub fingerprint: String,
	pub slots: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NestedPropPlacementDto {
	pub asset_id: AssetId,
	pub fingerprint: String,
	pub name: String,
	pub parent: EntityKey,
	pub local: TransformData,
	pub bindings: BTreeMap<String, NestedBinding>,
}

impl EditorWorkspace {
	pub fn nested_prop_source(
		&self,
		asset_id: &AssetId,
		fingerprint: &str,
	) -> Result<NestedPropSourceDto, String> {
		let source = self.checked_nested_prop(asset_id, fingerprint)?;
		Ok(NestedPropSourceDto {
			asset_id: source.source_id,
			fingerprint: source.fingerprint,
			slots: source.prefab.reference_slots.into_keys().collect(),
		})
	}

	pub fn add_prop_draft_placement(
		&mut self,
		id: &str,
		expected_revision: u64,
		request: NestedPropPlacementDto,
	) -> Result<PrefabSourceDraftDto, String> {
		let mut next = self.editable_prop_draft(id, expected_revision)?.clone();
		let source = self.checked_nested_prop(&request.asset_id, &request.fingerprint)?;
		let target = PinnedAssetVersion {
			asset_id: request.asset_id,
			fingerprint: request.fingerprint,
		};
		let placement = NestedPlacement {
			id: PlacementId::generate(),
			name: request.name,
			target,
			parent: request.parent,
			local: request.local,
			bindings: request.bindings,
		};
		let document = PrefabData::from_json(next.history.document())?;
		let candidate = document.with_nested_placement(&next.source_id, &source, placement)?;
		next.history
			.apply(expected_revision, candidate.to_json()?.into())?;
		self.install_prop_draft(id, next)
	}

	fn checked_nested_prop(
		&self,
		asset_id: &AssetId,
		fingerprint: &str,
	) -> Result<LinkedPrefabSource, String> {
		if self.backend.session().is_playing() {
			return Err("Stop Play before reviewing a nested Prop".into());
		}
		let catalog = runtime::assets::project_assets::load_catalog(&self.asset_root)?;
		let asset = catalog
			.by_id(asset_id)
			.filter(|entry| entry.kind == AssetKind::Prefab)
			.ok_or("The selected nested Prop is no longer registered")?;
		let source = self.prefab_source(&asset.path)?;
		if &source.source_id != asset_id || source.fingerprint != fingerprint {
			return Err(
				"The selected nested Prop changed. Refresh Assets and select it again.".into(),
			);
		}
		Ok(source)
	}
}
