//! Resolve catalog identities and freeze catalog input and output.

use engine_core::asset_catalog::{
	AssetCatalog, AssetId, AssetKind, CATALOG_FILE, MAX_CATALOG_BYTES,
};

use super::{
	Closure, Digest, PackageAsset, Path, Read, SceneData, Sha256, checked_asset_path, fs, io,
};

impl Closure<'_> {
	pub(super) fn record(&mut self, id: &AssetId) -> Result<(), String> {
		if !self.ids.insert(id.clone()) {
			return Ok(());
		}
		if self.depth == 64 {
			return Err("Asset dependency nesting exceeds 64 levels".into());
		}
		self.depth += 1;
		let result = self.record_contents(id);
		self.depth -= 1;
		result
	}

	fn record_contents(&mut self, id: &AssetId) -> Result<(), String> {
		let asset = self
			.catalog
			.by_id(id)
			.ok_or("Unresolved dependency asset identity")?
			.clone();
		let bytes = self.file(Path::new(&asset.path))?;
		if bytes.len() as u64 != asset.bytes
			|| format!("{:x}", Sha256::digest(&bytes)) != asset.fingerprint
		{
			return Err(format!(
				"Asset {} changed outside its catalog; reimport it before export",
				asset.path
			));
		}
		for dependency in &asset.dependencies {
			self.record(dependency)?;
		}
		match asset.kind {
			AssetKind::Model => {
				self.model(Path::new(&asset.path), None)?;
			}
			AssetKind::Prefab => self.prefab(Path::new(&asset.path))?,
			AssetKind::InputActions => {
				engine_core::input::InputActionMap::from_json(&bytes)?;
			}
			AssetKind::Scene => {
				let scene = SceneData::from_json(
					std::str::from_utf8(&bytes).map_err(|error| error.to_string())?,
				)?;
				self.entities(&scene.entities)?;
			}
			AssetKind::Texture => {}
		}
		Ok(())
	}

	pub(super) fn export_catalog(&mut self) -> Result<Vec<u8>, String> {
		let mut exported = self.catalog.clone();
		exported.assets.retain(|asset| self.ids.contains(&asset.id));
		for asset in &mut exported.assets {
			asset.source_path = None;
			if let Ok(path) = checked_asset_path(self.root, Path::new(&asset.path))
				&& let Ok(relative) = path.strip_prefix(self.root)
				&& let Some(bytes) = self.generated_files.get(relative)
			{
				asset.bytes = bytes.len() as u64;
				asset.fingerprint = format!("{:x}", Sha256::digest(bytes));
			}
		}
		let bytes = exported.to_json()?.into_bytes();
		if self.files.len() == 10_000 || self.bytes + bytes.len() as u64 > 512 * 1024 * 1024 {
			return Err("Export catalog exceeds the package file/byte budget".into());
		}
		self.files.insert(
			CATALOG_FILE.into(),
			PackageAsset {
				relative: CATALOG_FILE.into(),
				bytes: bytes.len() as u64,
				fingerprint: Sha256::digest(&bytes).into(),
			},
		);
		Ok(bytes)
	}
}

pub(crate) fn read_catalog(root: &Path) -> Result<AssetCatalog, String> {
	let relative = Path::new(CATALOG_FILE);
	match fs::symlink_metadata(root.join(relative)) {
		Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(AssetCatalog::default()),
		Err(error) => Err(error.to_string()),
		Ok(_) => {
			let path = checked_asset_path(root, relative).map_err(|error| error.to_string())?;
			let mut bytes = String::new();
			fs::File::open(path)
				.map_err(|error| error.to_string())?
				.take(MAX_CATALOG_BYTES as u64 + 1)
				.read_to_string(&mut bytes)
				.map_err(|error| error.to_string())?;
			AssetCatalog::from_json(&bytes)
		}
	}
}

pub(super) fn capture_catalog(root: &Path) -> Result<(AssetCatalog, Option<PackageAsset>), String> {
	let snapshot = if root.join(CATALOG_FILE).exists() {
		let path =
			checked_asset_path(root, Path::new(CATALOG_FILE)).map_err(|error| error.to_string())?;
		let mut bytes = Vec::new();
		fs::File::open(path)
			.map_err(|error| error.to_string())?
			.take(MAX_CATALOG_BYTES as u64 + 1)
			.read_to_end(&mut bytes)
			.map_err(|error| error.to_string())?;
		let catalog = AssetCatalog::from_json(
			std::str::from_utf8(&bytes).map_err(|error| error.to_string())?,
		)?;
		let snapshot = PackageAsset {
			relative: CATALOG_FILE.into(),
			bytes: bytes.len() as u64,
			fingerprint: Sha256::digest(&bytes).into(),
		};
		(catalog, Some(snapshot))
	} else {
		(AssetCatalog::default(), None)
	};
	Ok(snapshot)
}
