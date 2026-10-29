mod commands;
mod editor;
mod layout_preferences;
mod project_location;
mod state;
mod viewport;

use crate::commands::editor::*;
use crate::commands::engine::engine_status;
use crate::commands::preferences::{
	editor_layout_preferences_load, editor_layout_preferences_save,
};
use crate::commands::project::{choose_project_destination, create_project};
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
			create_project,
			choose_project_destination,
			editor_layout_preferences_load,
			editor_layout_preferences_save,
			editor_capabilities,
			editor_open,
			editor_open_project,
			editor_register_scene,
			editor_save_scene_as,
			editor_snapshot,
			editor_execute,
			editor_execute_incremental,
			editor_execute_receipt,
			editor_changes,
			editor_save_project,
			editor_build_project,
			editor_submit_import,
			editor_cancel_import,
			editor_drain_imports,
			editor_save,
			editor_reload,
			editor_assets,
			editor_import_model,
			editor_export_prefab,
			editor_instantiate_prefab,
			editor_save_prefab,
			editor_load_prefab,
			editor_collisions,
			editor_viewport_attach,
			editor_viewport_bounds,
			editor_viewport_detach,
			editor_viewport_status,
		])
		.run(tauri::generate_context!())
		.expect("error while running Game Engine editor");
	engine_logger::info!("Editor stopped");
	engine_logger::component_health("editor-host", "STOPPED", false, "Client host stopped");
}
