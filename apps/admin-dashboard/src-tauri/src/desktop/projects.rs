use super::{Service, error};
use crate::{AdminService, builder};
use tauri::State;

#[tauri::command]
pub(super) async fn preview_project(
	service: State<'_, Service>,
	input: builder::Input,
) -> Result<builder::Preview, String> {
	if input.source.trim().is_empty() {
		return Err("Choose a source checkout to preview project files.".into());
	}
	let request = service
		.lock()
		.unwrap_or_else(|e| e.into_inner())
		.builder
		.begin_preview(input)
		.map_err(error)?;
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
	service: State<'_, Service>,
	id: String,
) -> Result<builder::Generation, String> {
	let request = service
		.lock()
		.unwrap_or_else(|e| e.into_inner())
		.builder
		.begin_generation(&id)
		.map_err(error)?;
	let prepared =
		tauri::async_runtime::spawn_blocking(move || builder::Builder::prepare_generation(request))
			.await
			.map_err(|_| "Project generation worker failed; staging retained".to_owned())?
			.map_err(error)?;
	let mut service = service.lock().unwrap_or_else(|e| e.into_inner());
	let AdminService { builder, jobs, .. } = &mut *service;
	builder.finish_generation(prepared, jobs).map_err(error)
}
