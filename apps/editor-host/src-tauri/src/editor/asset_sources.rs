//! External sources explicitly selected by the user for this workspace.
use std::{
	collections::HashMap,
	path::{Path, PathBuf},
};

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(crate) enum AssetSourceKind {
	Glb,
	Wav,
	SkyPng,
}

#[derive(Default)]
pub(crate) struct AssetSources {
	selected: HashMap<AssetSourceKind, PathBuf>,
}

impl AssetSources {
	pub(crate) fn select(&mut self, kind: AssetSourceKind, path: PathBuf) {
		self.selected.insert(kind, path);
	}

	pub(crate) fn require(&self, kind: AssetSourceKind, requested: &Path) -> Result<(), String> {
		if self
			.selected
			.get(&kind)
			.is_some_and(|path| path == requested)
		{
			Ok(())
		} else {
			Err("Choose the source file in this project's file picker".into())
		}
	}
}
