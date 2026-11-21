use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::Path;
use std::sync::Mutex;

static STORAGE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct LayoutPreferences {
	pub schema_version: u32,
	pub components: BTreeMap<String, Map<String, Value>>,
}

impl LayoutPreferences {
	fn validate(&self) -> Result<(), String> {
		if self.schema_version != 1 {
			return Err(format!(
				"Unsupported layout preferences version: {}",
				self.schema_version
			));
		}
		Ok(())
	}
}

pub fn load(path: &Path) -> Result<Option<LayoutPreferences>, String> {
	let _guard = STORAGE_LOCK.lock().map_err(|error| error.to_string())?;
	let bytes = match fs::read(path) {
		Ok(bytes) => bytes,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
		Err(error) => return Err(format!("Cannot read layout preferences: {error}")),
	};
	let preferences: LayoutPreferences = serde_json::from_slice(&bytes)
		.map_err(|error| format!("Invalid layout preferences: {error}"))?;
	preferences.validate()?;
	Ok(Some(preferences))
}

pub fn save(path: &Path, preferences: &LayoutPreferences) -> Result<(), String> {
	preferences.validate()?;
	let bytes = serde_json::to_vec_pretty(preferences).map_err(|error| error.to_string())?;
	let _guard = STORAGE_LOCK.lock().map_err(|error| error.to_string())?;
	let parent = path
		.parent()
		.ok_or("Layout preferences path has no parent")?;
	fs::create_dir_all(parent).map_err(|error| error.to_string())?;
	let temporary = path.with_extension("json.tmp");
	let result = (|| -> std::io::Result<()> {
		let mut file = fs::File::create(&temporary)?;
		file.write_all(&bytes)?;
		file.sync_all()?;
		drop(file);
		fs::rename(&temporary, path)
	})();
	if result.is_err() {
		let _ = fs::remove_file(&temporary);
	}
	result.map_err(|error| format!("Cannot save layout preferences: {error}"))
}

#[cfg(test)]
#[path = "../tests/unit/layout_preferences.rs"]
mod tests;
