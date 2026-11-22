use super::*;
use crate::processes::Processes;
fn fixture(label: &str) -> (PathBuf, PathBuf) {
	let root = crate::tests::test_directory(&format!("builder-{label}"));
	let source = root.join("source");
	fs::create_dir_all(&source).unwrap();
	for (folder, name) in [
		("apps/game", "game"),
		("crates/logger", "engine-logger"),
		("crates/renderer", "renderer"),
		("crates/runtime", "runtime"),
		("crates/bridge", "bridge"),
		("crates/engine-core", "engine-core"),
		("crates/gameplay-catalog", "gameplay-catalog"),
		("crates/cinder-oath-gameplay", "cinder-oath-gameplay"),
		("crates/courtyard-gameplay", "courtyard-gameplay"),
	] {
		fs::create_dir_all(source.join(folder).join("src")).unwrap();
		fs::write(
			source.join(folder).join("Cargo.toml"),
			format!("[package]\nname='{name}'\nversion='0.1.0'\nedition='2021'\n"),
		)
		.unwrap();
		fs::write(
			source
				.join(folder)
				.join("src")
				.join(if name == "game" { "main.rs" } else { "lib.rs" }),
			if name == "game" {
				"fn main() {}"
			} else {
				"pub fn ready() {}"
			},
		)
		.unwrap();
	}
	let assets = source.join("projects/cinder-oath/assets");
	fs::create_dir_all(&assets).unwrap();
	fs::write(assets.join("sample.png"), b"fixture").unwrap();
	fs::write(assets.join(".env"), b"excluded").unwrap();
	fs::create_dir_all(source.join("crates/renderer/shaders")).unwrap();
	fs::write(source.join("crates/renderer/build.rs"), "fn main() {}").unwrap();
	(root, source)
}
fn input(source: &Path, destination: &Path) -> Input {
	Input {
		name: "fixture-game".into(),
		source: source.to_string_lossy().into_owned(),
		destination: destination.to_string_lossy().into_owned(),
		profile: "dev".into(),
		target: "local".into(),
		offline: true,
	}
}

mod generation;
mod preview;
