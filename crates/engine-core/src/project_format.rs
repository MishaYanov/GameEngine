use crate::project::{BehaviorSpec, MAX_PROJECT_BYTES, PROJECT_SCHEMA_VERSION, ProjectData};
use serde::Deserialize;
use std::{
	fs::File,
	io::{Read, Write},
	path::Path,
};

#[derive(Deserialize)]
pub(crate) struct VersionHeader {
	pub schema_version: u32,
}

impl ProjectData {
	pub fn from_json(source: &str) -> Result<Self, String> {
		if source.len() > MAX_PROJECT_BYTES {
			return Err("Project JSON exceeds 1 MiB".into());
		}
		let header: VersionHeader =
			serde_json::from_str(source).map_err(|e| format!("Invalid project JSON: {e}"))?;
		if header.schema_version != 2 && header.schema_version != PROJECT_SCHEMA_VERSION {
			return Err(format!(
				"Unsupported project schema version: {}",
				header.schema_version
			));
		}
		let mut value: Self = serde_json::from_str(source)
			.map_err(|error| format!("Invalid project JSON: {error}"))?;
		if header.schema_version == 2 {
			let fields: serde_json::Value = serde_json::from_str(source)
				.map_err(|error| format!("Invalid project JSON: {error}"))?;
			if fields.get("input_profile").is_some() {
				return Err("An authored input profile requires project schema version 3".into());
			}
			if fields.get("physics_layers").is_some() {
				return Err("Physics layers require project schema version 3".into());
			}
			if fields.get("scene_routes").is_some() || fields.get("sequences").is_some() {
				return Err("Scene flow and sequences require project schema version 3".into());
			}
			if fields.get("gameplay_module").is_some() {
				return Err("gameplay_module requires project schema version 3".into());
			}
			if fields.get("behavior_definitions").is_some()
				|| value
					.behaviors
					.iter()
					.any(|entry| matches!(&entry.behavior, BehaviorSpec::Shared { .. }))
			{
				return Err(
					"Shared behaviors and behavior_definitions require project schema version 3"
						.into(),
				);
			}
			value.schema_version = PROJECT_SCHEMA_VERSION;
		}
		value.validate()?;
		if header.schema_version != PROJECT_SCHEMA_VERSION {
			engine_logger::debug!(target: "engine_core::project", "Project schema migrated: from={}, to={PROJECT_SCHEMA_VERSION}", header.schema_version);
		}
		Ok(value)
	}

	pub fn to_json(&self) -> Result<String, String> {
		self.validate()?;
		let source = serde_json::to_string_pretty(self).map_err(|error| error.to_string())?;
		if source.len() > MAX_PROJECT_BYTES {
			return Err("Formatted project JSON exceeds 1 MiB".into());
		}
		Ok(source)
	}

	pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
		let file =
			File::open(path.as_ref()).map_err(|error| format!("Cannot read project: {error}"))?;
		let metadata = file.metadata().map_err(|error| error.to_string())?;
		if !metadata.is_file() {
			return Err("Project manifest must be a regular file".into());
		}
		if metadata.len() > MAX_PROJECT_BYTES as u64 {
			return Err("Project JSON exceeds 1 MiB".into());
		}
		let mut source = String::new();
		file.take(MAX_PROJECT_BYTES as u64 + 1)
			.read_to_string(&mut source)
			.map_err(|error| error.to_string())?;
		Self::from_json(&source)
	}
}

pub(crate) struct SizeBudget(pub(crate) usize);
impl Write for SizeBudget {
	fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
		self.0 = self
			.0
			.checked_add(bytes.len())
			.ok_or_else(|| std::io::Error::other("Project size overflow"))?;
		if self.0 > MAX_PROJECT_BYTES {
			return Err(std::io::Error::other("Project JSON exceeds 1 MiB"));
		}
		Ok(bytes.len())
	}
	fn flush(&mut self) -> std::io::Result<()> {
		Ok(())
	}
}
