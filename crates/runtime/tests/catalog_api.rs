use runtime::assets::catalog::scan_models;
use std::{
	fs,
	path::PathBuf,
	sync::atomic::{AtomicU64, Ordering},
};

struct Directory(PathBuf);
impl Directory {
	fn new() -> Self {
		static NEXT: AtomicU64 = AtomicU64::new(0);
		let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
			.join("../../target")
			.join(format!(
				"qa-catalog-{}-{}",
				std::process::id(),
				NEXT.fetch_add(1, Ordering::Relaxed)
			));
		fs::create_dir(&path).unwrap();
		Self(path)
	}
}
impl Drop for Directory {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

#[test]
fn catalog_filters_extensions_sorts_paths_and_returns_portable_relative_names() {
	let directory = Directory::new();
	fs::create_dir(directory.0.join("nested")).unwrap();
	for name in [
		"z.GLTF",
		"a.glb",
		"image.png",
		"not.glb.txt",
		"nested/robot.GLB",
		"nested/mesh.gltf",
	] {
		fs::write(directory.0.join(name), b"fixture").unwrap();
	}
	fs::create_dir(directory.0.join("folder.glb")).unwrap();
	let catalog = scan_models(directory.0.join("nested/..")).unwrap();
	assert_eq!(
		catalog
			.iter()
			.map(|asset| asset.path.as_str())
			.collect::<Vec<_>>(),
		vec!["a.glb", "nested/mesh.gltf", "nested/robot.GLB", "z.GLTF"]
	);
	assert_eq!(
		catalog
			.iter()
			.map(|asset| asset.name.as_str())
			.collect::<Vec<_>>(),
		vec!["a", "mesh", "robot", "z"]
	);
	assert_eq!(scan_models(&directory.0).unwrap(), catalog);
}

#[test]
fn missing_roots_and_excessive_directory_depth_fail_with_an_error() {
	let directory = Directory::new();
	assert!(scan_models(directory.0.join("missing")).is_err());
	let mut path = directory.0.clone();
	for _ in 0..33 {
		path = path.join("d");
		fs::create_dir(&path).unwrap();
	}
	assert!(scan_models(&directory.0).unwrap_err().contains("32"));
}

#[cfg(unix)]
#[test]
fn symlinks_to_files_directories_and_cycles_are_not_traversed() {
	use std::os::unix::fs::symlink;
	let directory = Directory::new();
	fs::create_dir(directory.0.join("models")).unwrap();
	fs::write(directory.0.join("models/real.glb"), b"fixture").unwrap();
	symlink("models/real.glb", directory.0.join("linked.glb")).unwrap();
	symlink("models", directory.0.join("linked-directory")).unwrap();
	symlink("..", directory.0.join("models/cycle")).unwrap();
	let assets = scan_models(&directory.0).unwrap();
	assert_eq!(assets.len(), 1);
	assert_eq!(assets[0].path, "models/real.glb");
}
