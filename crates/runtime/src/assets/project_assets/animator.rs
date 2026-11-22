use super::*;
use engine_core::animation::{
	ANIMATOR_GRAPH_SUFFIX, AnimationClipRef, AnimatorGraph, MAX_ANIMATOR_GRAPH_BYTES,
};
use std::collections::BTreeSet;

pub fn prepare_animator(
	asset_root: &Path,
	destination: &str,
	graph: &AnimatorGraph,
	replacement: Option<(&AssetId, &str)>,
) -> Result<PreparedAssetChange, String> {
	if !destination
		.to_ascii_lowercase()
		.ends_with(ANIMATOR_GRAPH_SUFFIX)
	{
		return Err("Animator assets use .animator.json".into());
	}
	let root = root(asset_root)?;
	let target = asset_path(&root, destination, false)?;
	let bytes = graph.to_json()?;
	let original_catalog = catalog_bytes(&root)?;
	let mut catalog = load_catalog(&root)?;
	let old = catalog.by_path(destination);
	let original_asset = match replacement {
		None if old.is_none() && !target.exists() => None,
		Some((id, fingerprint_expected)) => {
			let previous = old
				.filter(|asset| {
					asset.kind == AssetKind::AnimatorGraph
						&& &asset.id == id && asset.path == destination
						&& asset.fingerprint == fingerprint_expected
				})
				.ok_or("Animator changed since it was opened")?;
			let disk = read(&target, MAX_ANIMATOR_GRAPH_BYTES)?;
			if fingerprint(&disk) != previous.fingerprint {
				return Err("Animator changed on disk after review".into());
			}
			Some((target, fingerprint_expected.to_owned()))
		}
		None => return Err("Choose an unused path for the new Animator".into()),
	};
	let mut dependencies = BTreeSet::new();
	for state in &graph.states {
		let (id, fingerprint, kind) = match &state.clip {
			AnimationClipRef::Imported { source, .. } => {
				(&source.asset_id, &source.expected_sha256, AssetKind::Model)
			}
			AnimationClipRef::Authored {
				asset_id,
				expected_sha256,
			} => (asset_id, expected_sha256, AssetKind::AnimationClip),
		};
		catalog
			.by_id(id)
			.filter(|asset| asset.kind == kind && &asset.fingerprint == fingerprint)
			.ok_or_else(|| {
				format!(
					"Animator state {:?} refers to a missing or changed clip revision",
					state.id
				)
			})?;
		dependencies.insert(id.clone());
	}
	let asset = PersistentAsset {
		id: old.map_or_else(AssetId::generate, |asset| asset.id.clone()),
		path: destination.into(),
		kind: AssetKind::AnimatorGraph,
		aliases: old.map_or_else(Vec::new, |asset| asset.aliases.clone()),
		revision: old.map_or(Ok(1), |asset| {
			asset
				.revision
				.checked_add(1)
				.ok_or("Animator revision exhausted")
		})?,
		fingerprint: fingerprint(&bytes),
		bytes: bytes.len() as u64,
		source_path: None,
		meshes: Vec::new(),
		dependencies: dependencies.into_iter().collect(),
	};
	catalog.replace(asset.clone())?;
	let candidate = unique(&root, "candidate.tmp");
	write_new(&candidate, &bytes)?;
	Ok(PreparedAssetChange {
		root,
		operation_id: AssetId::generate().as_str().into(),
		candidate,
		destination: destination.into(),
		original_catalog,
		reimport: original_asset.is_some(),
		original_asset,
		catalog,
		asset,
	})
}
