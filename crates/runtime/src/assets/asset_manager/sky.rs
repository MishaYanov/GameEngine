use super::{AssetManager, ImageAsset, WindowRenderer};
use crate::assets::project_assets;
use engine_core::asset_catalog::{AssetId, CATALOG_FILE, PersistentAsset};
use renderer::vulkan::VulkanTexture;
use std::{
	fs,
	path::Path,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
		mpsc::{self, Receiver, TryRecvError},
	},
	time::{Duration, Instant, SystemTime},
};

pub(super) struct SkyTextureSlot {
	id: AssetId,
	checked: Option<Instant>,
	catalog_stamp: Option<FileStamp>,
	asset: Option<PersistentAsset>,
	file_stamp: Option<FileStamp>,
	texture: Option<Arc<VulkanTexture>>,
	needs_decode: bool,
}

#[derive(Clone, PartialEq, Eq)]
struct FileStamp {
	modified: Option<SystemTime>,
	bytes: u64,
}

impl FileStamp {
	fn read(path: &Path) -> Option<Self> {
		let metadata = fs::symlink_metadata(path).ok()?;
		metadata.is_file().then(|| Self {
			modified: metadata.modified().ok(),
			bytes: metadata.len(),
		})
	}
}

static SKY_DECODER_BUSY: AtomicBool = AtomicBool::new(false);
const DECODE_POLL: Duration = Duration::from_millis(20);

pub(super) struct SkyDecode {
	asset: PersistentAsset,
	stamp: FileStamp,
	result: Receiver<Result<ImageAsset, String>>,
}

struct DecodePermit;

impl Drop for DecodePermit {
	fn drop(&mut self) {
		SKY_DECODER_BUSY.store(false, Ordering::Release);
	}
}

impl SkyDecode {
	fn start(
		root: &Path,
		asset: &PersistentAsset,
		stamp: &FileStamp,
	) -> Result<Option<Self>, String> {
		if SKY_DECODER_BUSY
			.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
			.is_err()
		{
			return Ok(None);
		}
		let permit = DecodePermit;
		let (sender, result) = mpsc::sync_channel(1);
		let path = root.to_path_buf();
		let input = asset.clone();
		std::thread::Builder::new()
			.name("sky-panorama-decode".into())
			.spawn(move || {
				let _permit = permit;
				let image =
					project_assets::read_sky_panorama(&path, &input).map(|(_, image)| image);
				let _ = sender.send(image);
			})
			.map_err(|error| error.to_string())?;
		Ok(Some(Self {
			asset: asset.clone(),
			stamp: stamp.clone(),
			result,
		}))
	}

	fn matches(&self, slot: &SkyTextureSlot) -> bool {
		slot.asset.as_ref() == Some(&self.asset) && slot.file_stamp.as_ref() == Some(&self.stamp)
	}
}

impl AssetManager {
	pub fn sky_update_deadline(&self) -> Option<Instant> {
		self.sky_update_deadline
	}

	/// Bypass the metadata interval for the currently retained sky only.
	pub fn refresh_active_sky(&mut self) -> bool {
		let Some(slot) = &mut self.sky_texture else {
			return false;
		};
		slot.checked = None;
		true
	}

	/// Keep at most one authored sky texture per native asset owner. The renderer
	/// separately retains the previous texture until Vulkan finishes sampling it.
	pub fn sky_texture(
		&mut self,
		renderer: &WindowRenderer,
		image: Option<&AssetId>,
	) -> Option<Arc<VulkanTexture>> {
		let Some(image) = image else {
			self.sky_texture = None;
			self.sky_decode = None;
			self.sky_update_deadline = None;
			return None;
		};
		let mut slot = self
			.sky_texture
			.take()
			.filter(|slot| &slot.id == image)
			.unwrap_or_else(|| SkyTextureSlot {
				id: image.clone(),
				checked: None,
				catalog_stamp: None,
				asset: None,
				file_stamp: None,
				texture: None,
				needs_decode: false,
			});
		if slot
			.checked
			.is_none_or(|checked| checked.elapsed() >= Duration::from_secs(1))
		{
			let catalog_stamp = FileStamp::read(&self.root.join(CATALOG_FILE));
			let refresh_catalog = slot.checked.is_none() || slot.catalog_stamp != catalog_stamp;
			slot.checked = Some(Instant::now());
			let asset_changed = if refresh_catalog {
				slot.catalog_stamp = catalog_stamp;
				let asset = project_assets::load_catalog(&self.root)
					.ok()
					.and_then(|catalog| catalog.sky_panorama(image).ok().cloned());
				let changed = slot.asset != asset;
				slot.asset = asset;
				changed
			} else {
				false
			};
			let stamp = slot
				.asset
				.as_ref()
				.and_then(|asset| bridge::sky_panorama::project_path(&self.root, &asset.path).ok())
				.and_then(|path| FileStamp::read(&path));
			if asset_changed || stamp != slot.file_stamp {
				slot.texture = None;
				slot.file_stamp = stamp;
				slot.needs_decode = slot.asset.is_some() && slot.file_stamp.is_some();
			}
		}
		if let Some(job) = self.sky_decode.take().filter(|job| job.matches(&slot)) {
			match job.result.try_recv() {
				Err(TryRecvError::Empty) => self.sky_decode = Some(job),
				result => {
					slot.needs_decode = false;
					let path = &job.asset.path;
					let outcome = result
						.map_err(|error| error.to_string())
						.and_then(|image| image)
						.and_then(|image| self.upload_sky_texture(renderer, path, &image));
					match outcome {
						Ok(texture) => slot.texture = Some(texture),
						Err(error) => {
							engine_logger::warn!(target: "sky", "Sky Panorama {path}: {error}")
						}
					}
				}
			}
		}
		if slot.needs_decode
			&& self.sky_decode.is_none()
			&& let Some((asset, stamp)) = slot.asset.as_ref().zip(slot.file_stamp.as_ref())
		{
			match SkyDecode::start(&self.root, asset, stamp) {
				Ok(job) => self.sky_decode = job,
				Err(error) => {
					slot.needs_decode = false;
					engine_logger::warn!(target: "sky", "Sky Panorama {}: {error}", asset.path);
				}
			}
		}
		self.sky_update_deadline = slot.needs_decode.then(|| Instant::now() + DECODE_POLL);
		let texture = slot.texture.clone();
		self.sky_texture = Some(slot);
		texture
	}

	fn upload_sky_texture(
		&mut self,
		renderer: &WindowRenderer,
		path: &str,
		image: &ImageAsset,
	) -> Result<Arc<VulkanTexture>, String> {
		let bytes = image.pixels().len();
		self.gpu_budget
			.admit(bytes, 1)
			.map_err(|error| error.to_string())?;
		let texture = Arc::new(
			renderer
				.upload_rgba8(image.width(), image.height(), image.pixels())
				.map_err(|error| error.to_string())?,
		);
		texture.set_debug_label(path);
		self.gpu_budget.track_texture(&texture, bytes);
		Ok(texture)
	}
}
