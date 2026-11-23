use super::*;
use bridge::audio_clip::{self, AudioClip};

pub fn load_audio_clip(asset_root: &Path, asset: &PersistentAsset) -> Result<AudioClip, String> {
	if asset.kind != AssetKind::AudioClip {
		return Err("Select an Audio Clip asset".into());
	}
	let clip = audio_clip::read_project(asset_root, &asset.path)?;
	if clip.wav_bytes().len() as u64 != asset.bytes
		|| fingerprint(clip.wav_bytes()) != asset.fingerprint
	{
		return Err(
			"Audio Clip file changed outside its catalog; import the reviewed WAV again".into(),
		);
	}
	Ok(clip)
}

pub fn prepare_audio_clip_import(
	asset_root: &Path,
	source: &Path,
	destination: &str,
) -> Result<PreparedAssetChange, String> {
	if !destination.to_ascii_lowercase().ends_with(".wav") {
		return Err("Audio Clip destinations use .wav".into());
	}
	let root = root(asset_root)?;
	let target = asset_path(&root, destination, false)?;
	let original_catalog = catalog_bytes(&root)?;
	let mut catalog = match &original_catalog {
		Some(bytes) => {
			AssetCatalog::from_json(std::str::from_utf8(bytes).map_err(|error| error.to_string())?)?
		}
		None => AssetCatalog::default(),
	};
	if target.exists() || catalog.by_path(destination).is_some() {
		return Err("Audio Clip destination is already in use".into());
	}
	let clip = audio_clip::read_external(source)?;
	let bytes = clip.wav_bytes();
	let asset = PersistentAsset {
		id: AssetId::generate(),
		path: destination.into(),
		kind: AssetKind::AudioClip,
		aliases: Vec::new(),
		revision: 1,
		fingerprint: fingerprint(bytes),
		bytes: bytes.len() as u64,
		source_path: None,
		meshes: Vec::new(),
		dependencies: Vec::new(),
	};
	catalog.replace(asset.clone())?;
	let candidate = unique(&root, "candidate.tmp");
	write_new(&candidate, bytes)?;
	Ok(PreparedAssetChange {
		root,
		operation_id: AssetId::generate().as_str().into(),
		candidate,
		destination: destination.into(),
		original_catalog,
		original_asset: None,
		reimport: false,
		catalog,
		asset,
	})
}
