//! Scan glTF mesh metadata and resolve payload URIs.

use engine_core::asset_catalog::CATALOG_FILE;

use super::{Closure, MAX_URI_EDGES, Path, PathBuf};

impl Closure<'_> {
	pub(super) fn capture_uri(&mut self, model: &Path, uri: &str) -> Result<(), String> {
		if self.uri_edges == MAX_URI_EDGES {
			return Err("Model dependency closure exceeds 100000 URI edges".into());
		}
		self.uri_edges += 1;
		if let Some(path) = uri_path(model, uri)? {
			if path == Path::new(CATALOG_FILE)
				|| (cfg!(windows) && path.to_string_lossy().eq_ignore_ascii_case(CATALOG_FILE))
			{
				return Err(
					"Model dependencies cannot reference reserved root asset-catalog.json metadata"
						.into(),
				);
			}
			let path = self.key(&path)?;
			if path == Path::new(CATALOG_FILE)
				|| (cfg!(windows) && path.to_string_lossy().eq_ignore_ascii_case(CATALOG_FILE))
			{
				return Err(
					"Model dependencies cannot reference reserved root asset-catalog.json metadata"
						.into(),
				);
			}
			// Capture source bytes once. Every later stage still verifies the
			// fingerprint before and during its exclusive output copy.
			if !self.files.contains_key(&path) {
				self.file(&path)?;
			}
		}
		Ok(())
	}

	pub(super) fn model(&mut self, relative: &Path, mesh: Option<usize>) -> Result<(), String> {
		let relative = self.key(relative)?;
		if let Some(count) = self.models.get(&relative) {
			return if mesh.unwrap_or(0) < *count {
				Ok(())
			} else {
				Err("Referenced mesh index is unavailable".into())
			};
		}
		let bytes = self.file(&relative)?;
		let model =
			gltf::Gltf::from_slice(&bytes).map_err(|error| format!("Invalid glTF: {error}"))?;
		let count = model.meshes().count();
		if count > 10_000 || model.buffers().count() > 10_000 || model.images().count() > 10_000 {
			return Err("Model dependency metadata exceeds 10000 entries".into());
		}
		if mesh.unwrap_or(0) >= count {
			return Err("Referenced mesh index is unavailable".into());
		}
		self.models.insert(relative.to_owned(), count);
		let mut uris = Vec::new();
		for buffer in model.buffers() {
			if let gltf::buffer::Source::Uri(uri) = buffer.source() {
				uris.push(uri);
			}
		}
		for image in model.images() {
			if let gltf::image::Source::Uri { uri, .. } = image.source() {
				uris.push(uri);
			}
		}
		for uri in uris {
			self.capture_uri(&relative, uri)?;
		}
		Ok(())
	}
}

fn uri_path(model: &Path, uri: &str) -> Result<Option<PathBuf>, String> {
	if uri.starts_with("data:") {
		return Ok(None);
	}
	let decoded = urlencoding::decode(uri).map_err(|error| error.to_string())?;
	if decoded.contains([':', '\\', '\0']) || Path::new(decoded.as_ref()).is_absolute() {
		return Err("Model dependency must use a relative contained URI".into());
	}
	let mut result = model.parent().unwrap_or(Path::new("")).to_path_buf();
	for component in Path::new(decoded.as_ref()).components() {
		match component {
			std::path::Component::Normal(value) => result.push(value),
			std::path::Component::CurDir => {}
			std::path::Component::ParentDir => {
				if !result.pop() {
					return Err("Model dependency escapes the asset root".into());
				}
			}
			_ => return Err("Model dependency must be relative".into()),
		}
	}
	Ok(Some(result))
}
