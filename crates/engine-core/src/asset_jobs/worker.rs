use super::{
	AssetJobError, AssetJobLimits, AssetJobResult, CpuAssetImporter, MAX_ASSET_ERROR_BYTES, Request,
};
use std::sync::{
	Arc,
	atomic::{AtomicBool, Ordering},
	mpsc::{Receiver, SyncSender},
};

pub(super) fn run<I: CpuAssetImporter>(
	importer: I,
	limits: AssetJobLimits,
	worker_shutdown: Arc<AtomicBool>,
	incoming: Receiver<Request>,
	completed: SyncSender<AssetJobResult<I::Output>>,
	on_ready: impl Fn(),
) {
	engine_logger::debug!(target: "engine_core::asset_jobs", "CPU asset worker started: max_pending={}, max_reserved_bytes={}", limits.max_pending_jobs, limits.max_reserved_bytes);
	while let Ok(request) = incoming.recv() {
		if worker_shutdown.load(Ordering::Acquire) {
			break;
		}
		let result = if request.context.is_cancelled() {
			Err(AssetJobError::Cancelled)
		} else {
			let imported = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
				importer.import(&request.reference, &request.context)
			}));
			if request.context.is_cancelled() {
				Err(AssetJobError::Cancelled)
			} else {
				match imported {
					Ok(Ok(asset)) if asset.bytes <= request.context.byte_limit() => Ok(asset),
					Ok(Ok(asset)) => {
						let limit = request.context.byte_limit();
						engine_logger::debug!(target: "engine_core::asset_jobs", "Asset result exceeded reservation: job={}, generation={}, bytes={}, limit={limit}", request.id.value(), request.context.generation(), asset.bytes);
						Err(AssetJobError::ByteLimit {
							bytes: asset.bytes,
							limit,
						})
					}
					Ok(Err(error)) => {
						engine_logger::debug!(target: "engine_core::asset_jobs", "Asset import failed: job={}, generation={}", request.id.value(), request.context.generation());
						Err(AssetJobError::Import(bounded_import_error(error)))
					}
					Err(_) => Err(AssetJobError::Panicked),
				}
			}
		};
		// The global pending count includes completed results, so this
		// channel has a slot for every accepted reservation.
		if completed
			.send(AssetJobResult {
				id: request.id,
				generation: request.context.generation(),
				reference: request.reference,
				result,
			})
			.is_err()
		{
			break;
		}
		let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(&on_ready));
	}
	engine_logger::debug!(target: "engine_core::asset_jobs", "CPU asset worker stopped");
}

fn bounded_import_error(mut message: String) -> String {
	if message.len() > MAX_ASSET_ERROR_BYTES {
		let mut end = MAX_ASSET_ERROR_BYTES;
		while !message.is_char_boundary(end) {
			end -= 1;
		}
		message.truncate(end);
	}
	// Truncation alone would retain the original potentially large allocation.
	message.into_boxed_str().into_string()
}
