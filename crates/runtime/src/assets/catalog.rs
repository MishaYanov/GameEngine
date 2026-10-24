use std::{fs, path::Path};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelAsset {
	pub name: String,
	pub path: String,
}

/// Discover supported models under the root. Links are not traversed and the
/// importer independently checks canonical containment when an asset is opened.
pub fn scan_models(root: impl AsRef<Path>) -> Result<Vec<ModelAsset>, String> {
	let root = root
		.as_ref()
		.canonicalize()
		.map_err(|error| error.to_string())?;
	let mut pending = vec![(root.clone(), 0)];
	let mut models = Vec::new();
	let mut visited = 0;
	while let Some((directory, depth)) = pending.pop() {
		if depth > 32 {
			return Err("Asset directory nesting exceeds 32 levels".into());
		}
		for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
			let entry = entry.map_err(|error| error.to_string())?;
			visited += 1;
			if visited > 10_000 {
				return Err("Asset catalog exceeds 10,000 entries".into());
			}
			let kind = entry.file_type().map_err(|error| error.to_string())?;
			if kind.is_symlink() {
				continue;
			}
			let path = entry
				.path()
				.canonicalize()
				.map_err(|error| error.to_string())?;
			if !path.starts_with(&root) {
				continue;
			}
			if kind.is_dir() {
				pending.push((path, depth + 1));
			} else if kind.is_file()
				&& path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
					s.eq_ignore_ascii_case("gltf") || s.eq_ignore_ascii_case("glb")
				}) {
				let relative = path
					.strip_prefix(&root)
					.map_err(|error| error.to_string())?
					.to_str()
					.ok_or("Model path must be UTF-8")?
					.replace('\\', "/");
				let name = path
					.file_stem()
					.and_then(|s| s.to_str())
					.ok_or("Model name must be UTF-8")?
					.to_string();
				models.push(ModelAsset {
					name,
					path: relative,
				});
			}
		}
	}
	models.sort_by(|a, b| a.path.cmp(&b.path));
	Ok(models)
}
