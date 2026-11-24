use std::collections::HashSet;

use engine_core::asset_catalog::{AssetCatalog, AssetId, AssetKind};

pub(super) fn prefab_dependencies(
	catalog: &AssetCatalog,
	prefab: &engine_core::editor::PrefabData,
) -> Result<Vec<AssetId>, String> {
	let mut dependencies = HashSet::new();
	for entity in prefab.expanded_entities()?.iter() {
		if let Some(image) = entity
			.sky
			.as_ref()
			.and_then(|sky| sky.settings.background.image())
		{
			dependencies.insert(catalog.sky_panorama(image)?.id.clone());
		}
		if let Some(clip) = entity
			.audio_source
			.as_ref()
			.and_then(|source| source.settings.clip.as_ref())
		{
			let asset = catalog.audio_clip(&clip.asset)?;
			dependencies.insert(asset.id.clone());
		}
		if let Some(animation) = &entity.animation {
			let asset = catalog
				.by_id(&animation.settings.source.asset_id)
				.ok_or("Prefab animation source is not registered")?;
			if asset.kind != AssetKind::Model {
				return Err("Prefab animation source must be a model".into());
			}
			dependencies.insert(asset.id.clone());
		}
		if let Some((asset, _)) = catalog.resolve_mesh_reference(&entity.model_reference()?)? {
			dependencies.insert(asset.id.clone());
		}
	}
	let mut dependencies: Vec<_> = dependencies.into_iter().collect();
	dependencies.sort();
	Ok(dependencies)
}
