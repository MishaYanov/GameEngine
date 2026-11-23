use super::*;
use crate::editor::audio_audition::{AudioAuditionDto, AudioAuditionTarget};
use runtime::{asset_catalog::AssetId, assets::project_assets};

static ACTIVE_READS: AtomicU64 = AtomicU64::new(0);

struct ReadSlot;

impl ReadSlot {
	fn acquire() -> Result<Self, String> {
		ACTIVE_READS
			.fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
				(active < 2).then_some(active + 1)
			})
			.map(|_| Self)
			.map_err(|_| "Audio Clip audition is busy; wait for an active read to finish".into())
	}
}

impl Drop for ReadSlot {
	fn drop(&mut self) {
		ACTIVE_READS.fetch_sub(1, Ordering::AcqRel);
	}
}

#[tauri::command]
pub async fn editor_audio_source_audition(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	target: AudioAuditionTarget,
	expected_generation: u64,
	expected_revision: u64,
	request_id: u64,
) -> Result<AudioAuditionDto, String> {
	trusted_window(&window)?;
	let slot = ReadSlot::acquire()?;
	let prepared = workspace(state.clone(), false, None, move |editor| {
		editor.prepare_audio_audition(target, expected_generation, expected_revision, request_id)
	})
	.await?;
	let token = prepared.token;
	let bytes = tauri::async_runtime::spawn_blocking(move || {
		let _slot = slot;
		Ok::<_, String>(
			project_assets::load_audio_clip(&prepared.root, &prepared.clip)?.into_audition_wav(),
		)
	})
	.await
	.map_err(|error| error.to_string())
	.and_then(|result| result);
	workspace(state, false, None, move |editor| match bytes {
		Ok(bytes) => editor.finish_audio_audition(&token, bytes),
		Err(error) => {
			editor.stop_audio_audition(&token);
			Err(error)
		}
	})
	.await
}

#[tauri::command]
pub async fn editor_audio_source_audition_stop(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	token: AssetId,
) -> Result<(), String> {
	trusted_window(&window)?;
	workspace(state, false, None, move |editor| {
		editor.stop_audio_audition(&token);
		Ok(())
	})
	.await
}
