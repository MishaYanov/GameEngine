use super::{Service, error};
use crate::collector::{LogFilter, LogPage};
use std::path::PathBuf;
use tauri::State;

#[tauri::command]
pub(super) async fn resource_metrics(
	service: State<'_, Service>,
	selected_id: Option<String>,
) -> Result<crate::metrics::MetricsPage, String> {
	let directory = service
		.lock()
		.map_err(|_| "Admin state lock is unavailable")?
		.collector
		.directory
		.clone();
	tauri::async_runtime::spawn_blocking(move || {
		crate::metrics::read(&directory, selected_id.as_deref())
	})
	.await
	.map_err(|_| "Resource metrics reader failed".into())
}
#[tauri::command]
pub(super) fn log_page(
	service: State<Service>,
	after: u64,
	limit: usize,
	filter: LogFilter,
) -> LogPage {
	service
		.lock()
		.unwrap_or_else(|e| e.into_inner())
		.collector
		.page(after, limit, &filter)
}
#[derive(serde::Serialize)]
pub(super) struct ExportResult {
	path: String,
	count: usize,
}
#[tauri::command]
pub(super) fn export_logs(
	service: State<Service>,
	filter: LogFilter,
) -> Result<ExportResult, String> {
	let (path, count) = service
		.lock()
		.unwrap_or_else(|e| e.into_inner())
		.export(&filter)
		.map_err(error)?;
	Ok(ExportResult {
		path: path.to_string_lossy().into_owned(),
		count,
	})
}
#[tauri::command]
pub(super) fn open_source(service: State<Service>, cursor: u64) -> Result<(), String> {
	let (source, line) = service
		.lock()
		.unwrap_or_else(|e| e.into_inner())
		.source(cursor)
		.map_err(error)?;
	#[cfg(windows)]
	let candidates = [
		std::env::var_os("LOCALAPPDATA")
			.map(PathBuf::from)
			.unwrap_or_default()
			.join("Programs/Microsoft VS Code/Code.exe"),
		PathBuf::from("C:/Program Files/Microsoft VS Code/Code.exe"),
	];
	#[cfg(not(windows))]
	let candidates = [
		PathBuf::from("/usr/bin/code"),
		PathBuf::from("/usr/local/bin/code"),
	];
	let executable = candidates
		.into_iter()
		.find(|p| p.is_file())
		.ok_or("VS Code is unavailable; copy the source location instead")?;
	let mut command = std::process::Command::new(executable);
	command.args(["--goto", &format!("{}:{line}", source.display())]);
	crate::processes::hidden(&mut command);
	command.spawn().map_err(error)?;
	Ok(())
}
