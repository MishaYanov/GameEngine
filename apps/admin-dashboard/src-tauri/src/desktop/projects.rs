use super::{Service, error, trusted_window};
use crate::{AdminService, builder};
use std::path::Path;
use tauri::{State, WebviewWindow};

#[tauri::command]
pub(super) async fn preview_project(
	window: WebviewWindow,
	service: State<'_, Service>,
	mut input: builder::Input,
) -> Result<builder::Preview, String> {
	if input.source.trim().is_empty() {
		return Err("Choose a source checkout to preview project files.".into());
	}
	trusted_window(&window)?;
	let request = {
		let mut service = service.lock().unwrap_or_else(|e| e.into_inner());
		let approved = service.trusted_workspace().map_err(error)?;
		let source =
			crate::workspace_config::resolve_workspace(Path::new(&input.source)).map_err(error)?;
		if source != approved {
			return Err(
				"Choose the trusted source checkout under Overview > Workspace settings.".into(),
			);
		}
		input.source = approved.to_string_lossy().into_owned();
		service.builder.begin_preview(input).map_err(error)?
	};
	let prepared =
		tauri::async_runtime::spawn_blocking(move || builder::Builder::prepare_preview(request))
			.await
			.map_err(|_| "Project preview worker failed".to_owned())?
			.map_err(error)?;
	service
		.lock()
		.unwrap_or_else(|e| e.into_inner())
		.builder
		.finish_preview(prepared)
		.map_err(error)
}
#[tauri::command]
pub(super) async fn generate_project(
	window: WebviewWindow,
	service: State<'_, Service>,
	id: String,
) -> Result<builder::Generation, String> {
	trusted_window(&window)?;
	let request = {
		let mut service = service.lock().unwrap_or_else(|e| e.into_inner());
		service.trusted_workspace().map_err(error)?;
		service.builder.begin_generation(&id).map_err(error)?
	};
	let prepared =
		tauri::async_runtime::spawn_blocking(move || builder::Builder::prepare_generation(request))
			.await
			.map_err(|_| "Project generation worker failed; staging retained".to_owned())?
			.map_err(error)?;
	let mut service = service.lock().unwrap_or_else(|e| e.into_inner());
	let AdminService { builder, jobs, .. } = &mut *service;
	builder.finish_generation(prepared, jobs).map_err(error)
}
