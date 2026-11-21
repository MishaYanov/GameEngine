use engine_logger::Level;
use serde_json::Value;

pub(super) struct SourceLocation {
	pub(super) file: Option<String>,
	pub(super) line: Option<u32>,
}

pub(super) enum CargoOutput {
	Diagnostic {
		level: Level,
		message: String,
		source: Option<SourceLocation>,
	},
	Artifact {
		filenames: Vec<String>,
	},
}

impl CargoOutput {
	pub(super) fn parse(line: &str, package: &str) -> Option<Self> {
		let value = serde_json::from_str::<Value>(line).ok()?;
		match value["reason"].as_str()? {
			"compiler-message" => {
				let message = &value["message"];
				let level = match message["level"].as_str() {
					Some("error" | "failure-note") => Level::Error,
					Some("warning") => Level::Warning,
					_ => Level::Info,
				};
				let source = message["spans"]
					.as_array()
					.and_then(|spans| spans.iter().find(|span| span["is_primary"] == true))
					.map(|span| SourceLocation {
						file: span["file_name"].as_str().map(str::to_owned),
						line: span["line_start"]
							.as_u64()
							.and_then(|line| u32::try_from(line).ok()),
					});
				let message = message["rendered"]
					.as_str()
					.or(message["message"].as_str())
					.unwrap_or("Cargo diagnostic")
					.to_owned();
				Some(Self::Diagnostic {
					level,
					message,
					source,
				})
			}
			"compiler-artifact" => {
				let target = value["target"]["name"].as_str()?;
				if target != package && target != package.replace('-', "_") {
					return None;
				}
				let filenames = value["filenames"]
					.as_array()
					.into_iter()
					.flatten()
					.filter_map(|filename| filename.as_str().map(str::to_owned))
					.collect();
				Some(Self::Artifact { filenames })
			}
			_ => None,
		}
	}
}
