mod commands;
mod state;

use crate::commands::engine::engine_status;
use crate::commands::project::create_project;
use runtime::Engine;
use state::EditorState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _logging =
        engine_logger::init_application("editor-host").expect("failed to initialize editor logger");
    engine_logger::info!("Initializing editor engine");
    let engine = Engine::new().expect("failed to initialize Game Engine");
    engine_logger::component_health("editor-host", "HEALTHY", false, "Client host initialized");

    tauri::Builder::default()
        .manage(EditorState::new(engine))
        .invoke_handler(tauri::generate_handler![engine_status, create_project])
        .run(tauri::generate_context!())
        .expect("error while running Game Engine editor");
    engine_logger::info!("Editor stopped");
    engine_logger::component_health("editor-host", "STOPPED", false, "Client host stopped");
}
