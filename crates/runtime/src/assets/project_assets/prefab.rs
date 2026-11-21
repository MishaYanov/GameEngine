use std::collections::HashSet;

use engine_core::asset_catalog::{AssetCatalog, AssetId, AssetKind};

pub(super) fn prefab_dependencies(
	catalog: &AssetCatalog,
	prefab: &engine_core::editor::PrefabData,
) -> Result<Vec<AssetId>, String> {
	let mut dependencies = HashSet::new();
	for entity in &prefab.entities {
		if let Some(animation) = &entity.animation {
			let asset = catalog
				.by_id(&animation.settings.source.asset_id)
				.ok_or("Prefab animation source is not registered")?;
			if asset.kind != AssetKind::Model {
				return Err("Prefab animation source must be a model".into());
			}
			dependencies.insert(asset.id.clone());
		}
		if let Some(asset) = catalog.resolve(
			entity.asset_id.as_ref(),
			&entity.model_path,
			entity.mesh_index,
		)? {
			dependencies.insert(asset.id.clone());
		}
	}
	let mut dependencies: Vec<_> = dependencies.into_iter().collect();
	dependencies.sort();
	Ok(dependencies)
}
