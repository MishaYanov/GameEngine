use super::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);
impl TestDirectory {
	fn new() -> Self {
		let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
		let path = std::env::temp_dir().join(format!("editor-layout-{}-{id}", std::process::id()));
		fs::create_dir_all(&path).unwrap();
		Self(path)
	}
	fn file(&self) -> PathBuf {
		self.0.join("preferences").join("editor-layout.json")
	}
}
impl Drop for TestDirectory {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

fn preferences(width: &str) -> LayoutPreferences {
	serde_json::from_value(serde_json::json!({
		"schema_version": 1,
		"components": {"editor.navigation": {"width": width, "expanded": false}}
	}))
	.unwrap()
}

#[test]
fn missing_file_uses_defaults_and_saved_preferences_round_trip() {
	let directory = TestDirectory::new();
	let path = directory.file();
	assert_eq!(load(&path).unwrap(), None);
	save(&path, &preferences("25%")).unwrap();
	assert_eq!(load(&path).unwrap(), Some(preferences("25%")));
	save(&path, &preferences("40%")).unwrap();
	assert_eq!(load(&path).unwrap(), Some(preferences("40%")));
	assert!(!path.with_extension("json.tmp").exists());
}

#[test]
fn invalid_version_does_not_overwrite_saved_preferences() {
	let directory = TestDirectory::new();
	let path = directory.file();
	let original = preferences("25%");
	save(&path, &original).unwrap();
	let mut invalid = preferences("50%");
	invalid.schema_version = 2;
	assert!(save(&path, &invalid).is_err());
	assert_eq!(load(&path).unwrap(), Some(original));
}

#[test]
fn malformed_preferences_report_error() {
	let directory = TestDirectory::new();
	let path = directory.file();
	fs::create_dir_all(path.parent().unwrap()).unwrap();
	fs::write(&path, b"{broken").unwrap();
	assert!(load(&path)
		.unwrap_err()
		.contains("Invalid layout preferences"));
	fs::write(&path, br#"{"schema_version":2,"components":{}}"#).unwrap();
	assert!(load(&path).unwrap_err().contains("Unsupported"));
}
