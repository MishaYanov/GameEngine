use serde::Deserialize;

#[derive(Deserialize)]
pub(crate) struct VersionHeader {
	pub schema_version: u32,
}
