//! Trusted editor access to the project-owned gameplay feedback catalog.
use super::{trusted_window, workspace, EditorState, EditorWorkspace};
use bridge::feedback_audio::{self, CueAssignment, CueClip, FeedbackBusMix, PreparedCueCatalog};
use runtime::scene_io::{replace_bytes, save_new_bytes};
use serde::Serialize;
use std::{
	fs::{self, File, OpenOptions},
	io,
	path::Path,
};
use tauri::{State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

#[derive(Serialize)]
pub struct FeedbackCueDraftDto {
	workspace_generation: u64,
	catalog_digest: Option<String>,
	cues: Vec<CueAssignment>,
	mix: FeedbackBusMix,
}

impl FeedbackCueDraftDto {
	fn new(generation: u64, catalog: Option<&PreparedCueCatalog>) -> Self {
		Self {
			workspace_generation: generation,
			catalog_digest: catalog.map(PreparedCueCatalog::digest),
			cues: catalog.map_or_else(Vec::new, |catalog| catalog.assignments().collect()),
			mix: catalog.map_or_else(FeedbackBusMix::default, |catalog| catalog.mix().clone()),
		}
	}
}

#[tauri::command]
pub async fn editor_feedback_cues_read(
	window: WebviewWindow,
	state: State<'_, EditorState>,
) -> Result<FeedbackCueDraftDto, String> {
	trusted_window(&window)?;
	let (generation, catalog) =
		prepare_for_workspace(state.clone(), None, feedback_audio::load).await?;
	workspace(state, false, None, move |editor| {
		require_editing(editor, Some(generation))?;
		if let Some(catalog) = &catalog {
			validate_mix_scenes(editor, catalog)?;
		}
		Ok(FeedbackCueDraftDto::new(generation, catalog.as_ref()))
	})
	.await
}

#[tauri::command]
pub async fn editor_feedback_cue_import(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	workspace_generation: u64,
	slug: String,
) -> Result<Option<String>, String> {
	trusted_window(&window)?;
	feedback_audio::import_path(&slug)?;
	workspace(state.clone(), false, None, move |editor| {
		require_editing(editor, Some(workspace_generation))
	})
	.await?;
	let clip = tauri::async_runtime::spawn_blocking(move || {
		let Some(file) = window
			.dialog()
			.file()
			.set_parent(&window)
			.set_title("Import feedback WAV")
			.add_filter("WAV audio", &["wav"])
			.blocking_pick_file()
		else {
			return Ok(None);
		};
		trusted_window(&window)?;
		let source = file.into_path().map_err(|error| error.to_string())?;
		feedback_audio::read_external_clip(&source).map(Some)
	})
	.await
	.map_err(|error| error.to_string())??;
	let Some(clip) = clip else { return Ok(None) };
	workspace(state, false, Some("import_feedback_cue"), move |editor| {
		require_editing(editor, Some(workspace_generation))?;
		save_clip(&editor.asset_root, &slug, &clip).map(Some)
	})
	.await
}

#[tauri::command]
pub async fn editor_feedback_cues_apply(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	workspace_generation: u64,
	expected_digest: Option<String>,
	cues: Vec<CueAssignment>,
	mix: FeedbackBusMix,
) -> Result<FeedbackCueDraftDto, String> {
	trusted_window(&window)?;
	let (_, catalog) =
		prepare_for_workspace(state.clone(), Some(workspace_generation), move |root| {
			feedback_audio::prepare_catalog_with_mix(root, cues, mix)
		})
		.await?;
	workspace(state, false, Some("apply_feedback_cues"), move |editor| {
		require_editing(editor, Some(workspace_generation))?;
		validate_mix_scenes(editor, &catalog)?;
		publish_catalog(&editor.asset_root, expected_digest.as_deref(), &catalog)?;
		Ok(FeedbackCueDraftDto::new(editor.generation, Some(&catalog)))
	})
	.await
}

#[tauri::command]
pub async fn editor_feedback_cue_preview(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	workspace_generation: u64,
	relative_path: String,
) -> Result<Vec<u8>, String> {
	trusted_window(&window)?;
	let (_, clip) = prepare_for_workspace(state.clone(), Some(workspace_generation), move |root| {
		feedback_audio::load_clip(root, &relative_path)
	})
	.await?;
	workspace(state, false, None, move |editor| {
		require_editing(editor, Some(workspace_generation))?;
		Ok(clip.bytes().to_vec())
	})
	.await
}

async fn prepare_for_workspace<T: Send + 'static>(
	state: State<'_, EditorState>,
	expected: Option<u64>,
	prepare: impl FnOnce(&Path) -> Result<T, String> + Send + 'static,
) -> Result<(u64, T), String> {
	let (generation, root) = workspace(state, false, None, move |editor| {
		require_editing(editor, expected)?;
		Ok((editor.generation, editor.asset_root.clone()))
	})
	.await?;
	let prepared = tauri::async_runtime::spawn_blocking(move || prepare(&root))
		.await
		.map_err(|error| error.to_string())??;
	Ok((generation, prepared))
}

fn require_editing(editor: &EditorWorkspace, generation: Option<u64>) -> Result<(), String> {
	if generation.is_some_and(|generation| generation != editor.generation) {
		return Err("The project changed. Reopen Feedback Audio to continue.".into());
	}
	let session = editor.backend.session();
	if session.is_playing() || session.has_transaction() {
		return Err("Finish Play or the current edit before changing feedback audio.".into());
	}
	Ok(())
}

fn validate_mix_scenes(
	editor: &EditorWorkspace,
	catalog: &PreparedCueCatalog,
) -> Result<(), String> {
	catalog
		.mix()
		.validate_scenes(&editor.backend.session().project().scenes)
}

fn save_clip(asset_root: &Path, slug: &str, clip: &CueClip) -> Result<String, String> {
	let relative = feedback_audio::import_path(slug)?;
	let filename = Path::new(&relative)
		.file_name()
		.ok_or("Unexpected feedback cue path")?;
	let destination = feedback_audio::ensure_audio_directory(asset_root)?.join(filename);
	save_new_bytes(&destination, clip.bytes())?;
	Ok(relative)
}

#[cfg(test)]
fn apply_catalog(
	asset_root: &Path,
	expected_digest: Option<&str>,
	assignments: Vec<CueAssignment>,
) -> Result<PreparedCueCatalog, String> {
	let catalog = feedback_audio::prepare_catalog(asset_root, assignments)?;
	publish_catalog(asset_root, expected_digest, &catalog)?;
	Ok(catalog)
}

fn publish_catalog(
	asset_root: &Path,
	expected_digest: Option<&str>,
	catalog: &PreparedCueCatalog,
) -> Result<(), String> {
	let audio = feedback_audio::ensure_audio_directory(asset_root)?;
	let _catalog_lock = lock_catalog(&audio)?;
	let destination = audio.join("cues.json");
	if feedback_audio::catalog_digest(asset_root)?.as_deref() != expected_digest {
		return Err(
			"The feedback cue catalog changed. Reopen Feedback Audio and try again.".into(),
		);
	}
	if expected_digest.is_some() {
		replace_bytes(&destination, catalog.manifest())?;
	} else {
		save_new_bytes(&destination, catalog.manifest())?;
	}
	Ok(())
}

fn lock_catalog(audio: &Path) -> Result<File, String> {
	// Keep this coordination file beside the asset root, outside dependency export.
	let root = audio
		.parent()
		.ok_or("Feedback Audio asset root is unavailable")?;
	let project = root
		.parent()
		.ok_or("Feedback Audio project directory is unavailable")?;
	let mut name = root
		.file_name()
		.ok_or("Feedback Audio asset root has no name")?
		.to_os_string();
	name.push(".feedback-audio.lock");
	let path = project.join(name);
	let ordinary = |metadata: &fs::Metadata| {
		#[cfg(windows)]
		{
			use std::os::windows::fs::MetadataExt;
			if metadata.file_attributes() & 0x400 != 0 {
				return false;
			}
		}
		metadata.is_file()
	};
	match fs::symlink_metadata(&path) {
		Ok(metadata) if ordinary(&metadata) => {}
		Ok(_) => return Err("Feedback Audio lock must be an ordinary file".into()),
		Err(error) if error.kind() == io::ErrorKind::NotFound => {}
		Err(error) => return Err(error.to_string()),
	}
	let mut options = OpenOptions::new();
	options.read(true).write(true).create(true).truncate(false);
	#[cfg(windows)]
	{
		use std::os::windows::fs::OpenOptionsExt;
		const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
		options.custom_flags(OPEN_REPARSE_POINT);
	}
	let file = options.open(path).map_err(|error| error.to_string())?;
	if !ordinary(&file.metadata().map_err(|error| error.to_string())?) {
		return Err("Feedback Audio lock must be an ordinary file".into());
	}
	file.try_lock()
		.map_err(|_| "Another Editor is updating the feedback cue catalog")?;
	Ok(file)
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/editor/feedback_audio_tests.rs"]
mod tests;
