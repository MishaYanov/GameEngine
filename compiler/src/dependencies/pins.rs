//! Restore Clip owner provenance from an exact trusted pin when only the editing source is missing.
use super::{AssetId, AssetKind, Closure, Path, checked_asset_path, fs, io};
use engine_core::{
	SceneData,
	editor::PrefabData,
	linked_prefab::{PinnedPropSource, PropComposition},
};
use std::collections::{BTreeSet, HashMap};

const MAX_PINNED_OWNER_BYTES: usize = 128 * 1024 * 1024;

impl Closure<'_> {
	pub(super) fn capture_clip_owner_pins(&mut self, scene: &SceneData) -> Result<(), String> {
		for link in scene
			.entities
			.iter()
			.filter_map(|entity| entity.linked_prefab.as_ref())
		{
			let source = &link.source;
			if self.missing_clip_owner(&source.source_id, &source.fingerprint) {
				self.retain_clip_owner(&source.source_id, &source.prefab)?;
			}
			let pins = &source.prefab.composition.pins;
			for pin in pins {
				if self.missing_clip_owner(&pin.version.asset_id, &pin.version.fingerprint) {
					let prefab = prefab_for_pin(pin, pins)?;
					self.retain_clip_owner(&pin.version.asset_id, &prefab)?;
				}
			}
		}
		Ok(())
	}

	fn missing_clip_owner(&self, id: &AssetId, fingerprint: &str) -> bool {
		self.clip_owners.contains(id)
			&& self.catalog.by_id(id).is_some_and(|asset| {
				asset.kind == AssetKind::Prefab
					&& asset.fingerprint == fingerprint
					&& source_is_missing(self.root, Path::new(&asset.path))
			})
	}

	fn retain_clip_owner(&mut self, id: &AssetId, prefab: &PrefabData) -> Result<(), String> {
		let bytes = prefab.to_json()?.into_bytes();
		if let Some(previous) = self.pinned_clip_sources.get(id) {
			return if previous == &bytes {
				Ok(())
			} else {
				Err(format!(
					"Pinned editing sources disagree for Clip owner {}",
					id.as_str()
				))
			};
		}
		self.pinned_clip_bytes = self
			.pinned_clip_bytes
			.checked_add(bytes.len())
			.filter(|bytes| *bytes <= MAX_PINNED_OWNER_BYTES)
			.ok_or("Pinned Animation Clip owners exceed the 128 MiB provenance budget")?;
		self.pinned_clip_sources.insert(id.clone(), bytes);
		Ok(())
	}
}

fn source_is_missing(root: &Path, relative: &Path) -> bool {
	let mut path = root.to_path_buf();
	for component in relative.components() {
		path.push(component);
		match fs::symlink_metadata(&path) {
			Ok(_) => {
				let Ok(fragment) = path.strip_prefix(root) else {
					return false;
				};
				if checked_asset_path(root, fragment).is_err() {
					return false;
				}
			}
			Err(error) if error.kind() == io::ErrorKind::NotFound => return true,
			Err(_) => return false,
		}
	}
	false
}

fn prefab_for_pin(
	pin: &PinnedPropSource,
	captured: &[PinnedPropSource],
) -> Result<PrefabData, String> {
	let by_version: HashMap<_, _> = captured
		.iter()
		.map(|entry| (&entry.version, entry))
		.collect();
	let mut reachable = BTreeSet::new();
	let mut pending = pin
		.placements
		.iter()
		.map(|placement| &placement.target)
		.collect::<Vec<_>>();
	while let Some(version) = pending.pop() {
		if reachable.insert(version) {
			let source = by_version
				.get(version)
				.ok_or("Pinned Clip owner has an unresolved nested source")?;
			pending.extend(source.placements.iter().map(|placement| &placement.target));
		}
	}
	Ok(PrefabData {
		schema_version: if reachable.is_empty() { 3 } else { 4 },
		entities: pin.entities.clone(),
		gameplay: None,
		reference_slots: pin.reference_slots.clone(),
		composition: PropComposition {
			placements: pin.placements.clone(),
			pins: reachable
				.into_iter()
				.map(|version| by_version[version].clone())
				.collect(),
		},
	})
}
