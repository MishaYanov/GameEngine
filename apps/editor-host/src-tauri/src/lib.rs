mod animation_preview;
mod asset_reimport_gpu;
mod camera_preview;
mod commands;
mod editor;
mod layout_preferences;
mod project_location;
mod state;
mod viewport;
mod viewport_interaction;
mod viewport_tools;

use crate::commands::editor::*;
use crate::commands::engine::engine_status;
use crate::commands::preferences::{
	editor_layout_preferences_load, editor_layout_preferences_save,
};
use crate::commands::project::{
	choose_project_destination, choose_project_manifest, confirm_project_discard, create_project,
	editor_exit,
};
use crate::commands::runtime_inspection::editor_runtime_inspect;
use state::EditorState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
	let _logging =
		engine_logger::init_application("editor-host").expect("failed to initialize editor logger");
	engine_logger::info!("Initializing editor host; GPU resources are created on demand");
	engine_logger::component_health("editor-host", "HEALTHY", false, "Client host initialized");

	tauri::Builder::default()
		.plugin(tauri_plugin_dialog::init())
		.manage(EditorState::default())
		.setup(|app| {
			let parent = app
				.get_webview_window("main")
				.ok_or("Missing main editor window")?;
			let viewport = std::sync::Arc::clone(&app.state::<EditorState>().viewport);
			let window = parent.clone();
			parent.on_window_event(move |event| match event {
				tauri::WindowEvent::CloseRequested { api, .. } => {
					api.prevent_close();
					if let Ok(mut host) = viewport.lock() {
						host.close_parent_when_ready(window.clone());
					} else {
						engine_logger::error!("Cannot acquire viewport state during shutdown");
					}
				}
				tauri::WindowEvent::Resized(size) => {
					if let Ok(host) = viewport.lock() {
						host.set_parent_visible(
							size.width > 0
								&& size.height > 0 && !window.is_minimized().unwrap_or(false),
						);
					}
				}
				_ => {}
			});
			Ok(())
		})
		.invoke_handler(tauri::generate_handler![
			engine_status,
			editor_runtime_inspect,
			create_project,
			choose_project_destination,
			choose_project_manifest,
			confirm_project_discard,
			editor_exit,
			editor_layout_preferences_load,
			editor_layout_preferences_save,
			editor_capabilities,
			editor_animation_metadata,
			editor_animation_preview_open,
			editor_animation_workspace_open,
			editor_animation_clips,
			editor_open_authored_clip,
			editor_save_authored_clip,
			editor_authored_clip_preview_open,
			editor_authored_clip_preview_sample,
			editor_authored_clip_preview_close,
			editor_clip_timeline_draft_open,
			editor_clip_timeline_draft_edit,
			editor_clip_timeline_draft_save,
			editor_clip_timeline_draft_close,
			editor_clip_timeline_preview_open,
			editor_clip_timeline_preview_sample,
			editor_animation_preview_sample,
			editor_animation_preview_keys,
			editor_animation_preview_close,
			editor_camera_preview_open,
			editor_camera_preview_sample,
			editor_camera_preview_close,
			editor_prop_preview_open,
			editor_prop_preview_close,
			editor_prop_preview_frame,
			editor_prop_preview_select,
			editor_open,
			editor_open_project,
			editor_register_scene,
			editor_save_scene_as,
			editor_snapshot,
			editor_check_scenes,
			editor_execute,
			editor_execute_incremental,
			editor_execute_receipt,
			editor_changes,
			editor_save_project,
			editor_save_all,
			editor_build_project,
			editor_prepare_project_build,
			editor_finish_project_build,
			editor_run_build,
			editor_submit_import,
			editor_cancel_import,
			editor_drain_imports,
			editor_save,
			editor_reload,
			editor_assets,
			editor_import_model,
			editor_choose_external_glb,
			editor_choose_external_wav,
			editor_import_external_wav,
			editor_audio_clip_info,
			editor_audio_source_audition,
			editor_audio_source_audition_stop,
			editor_feedback_cues_read,
			editor_feedback_cue_import,
			editor_feedback_cues_apply,
			editor_feedback_cue_preview,
			editor_import_external_glb,
			editor_reimport_asset,
			editor_move_asset,
			editor_list_animators,
			editor_open_animator,
			editor_save_animator,
			editor_validate_animator_actor,
			editor_list_control_graphs,
			editor_open_control_graph,
			editor_save_control_graph,
			editor_probe_controls_graph,
			editor_list_input_actions,
			editor_open_input_actions,
			editor_save_input_actions,
			editor_move_input_actions,
			editor_export_prefab,
			editor_instantiate_prefab,
			editor_save_prefab,
			editor_load_prefab,
			editor_prefab_source_open,
			editor_prefab_source_close,
			editor_prefab_source_validate,
			editor_prefab_source_save,
			editor_prop_draft_edit,
			editor_prop_draft_save,
			editor_prop_nested_source,
			editor_prop_draft_add_nested,
			editor_prop_draft_refresh_nested,
			editor_review_linked_prop_overrides,
			editor_update_linked_prop_reviewed,
			editor_apply_linked_prop_override,
			editor_review_linked_prop_recovery,
			editor_recover_linked_prop,
			editor_create_linked_prefab_source,
			editor_instantiate_linked_prefab,
			editor_linked_prefab_status,
			editor_update_linked_prefab,
			editor_collisions,
			editor_viewport_attach,
			commands::editor::editor_warm_scene,
			commands::editor::editor_set_scene_warming_mode,
			commands::editor::editor_export_warming_report,
			editor_viewport_bounds,
			editor_viewport_detach,
			editor_viewport_status,
			editor_viewport_editor_state,
			editor_viewport_action,
		])
		.run(tauri::generate_context!())
		.expect("error while running Game Engine editor");
	engine_logger::info!("Editor stopped");
	engine_logger::component_health("editor-host", "STOPPED", false, "Client host stopped");
}
