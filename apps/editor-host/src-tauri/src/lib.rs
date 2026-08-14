mod commands;
mod state;

use runtime::Engine;
use state::EditorState;
use crate::commands::engine::engine_status;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let engine = Engine::new()
        .expect("failed to initialize Game Engine");

    tauri::Builder::default()
        .manage(EditorState::new(engine))
        .invoke_handler(
            tauri::generate_handler![
                engine_status,
            ]
        )
        .run(tauri::generate_context!())
        .expect("error while running Game Engine editor");
}