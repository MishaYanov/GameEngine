use crate::{
	AdminService, DashboardSnapshot,
	collector::{LogFilter, LogPage},
	processes::ProcessRun,
};
use std::{
	path::PathBuf,
	sync::{Arc, Mutex},
};
use tauri::State;
type Service = Arc<Mutex<AdminService>>;
fn error(error: impl ToString) -> String {
	error.to_string()
}
#[tauri::command]
fn dashboard_snapshot(service: State<Service>) -> DashboardSnapshot {
	service.lock().unwrap_or_else(|e| e.into_inner()).snapshot()
}
#[tauri::command]
fn log_page(service: State<Service>, after: u64, limit: usize, filter: LogFilter) -> LogPage {
	service
		.lock()
		.unwrap_or_else(|e| e.into_inner())
		.collector
		.page(after, limit, &filter)
}
#[tauri::command]
fn configure_workspace(
	service: State<Service>,
	workspace: String,
	logs: String,
) -> Result<(), String> {
	service
		.lock()
		.unwrap_or_else(|e| e.into_inner())
		.configure(&workspace, &logs)
		.map_err(error)
}
#[tauri::command]
fn start_application(service: State<Service>, id: String) -> Result<ProcessRun, String> {
	let mut service = service.lock().unwrap_or_else(|e| e.into_inner());
	let workspace = service.workspace.clone();
	let logs = service.collector.directory.clone();
	service
		.processes
		.start(&workspace, &logs, &id)
		.map_err(error)
}
#[tauri::command]
fn stop_application(service: State<Service>, id: String) -> Result<(), String> {
	service
		.lock()
		.unwrap_or_else(|e| e.into_inner())
		.processes
		.stop(&id)
		.map_err(error)
}
#[tauri::command]
fn restart_application(service: State<Service>, id: String) -> Result<ProcessRun, String> {
	let mut service = service.lock().unwrap_or_else(|e| e.into_inner());
	service.processes.stop(&id).map_err(error)?;
	let workspace = service.workspace.clone();
	let logs = service.collector.directory.clone();
	service
		.processes
		.start(&workspace, &logs, &id)
		.map_err(error)
}
#[tauri::command]
fn queue_job(
	service: State<Service>,
	kind: crate::jobs::Kind,
	package: String,
	profile: String,
) -> Result<crate::jobs::Run, String> {
	let mut service = service.lock().unwrap_or_else(|e| e.into_inner());
	let workspace = service.workspace.clone();
	service
		.jobs
		.queue(&workspace, kind, package, profile)
		.map_err(error)
}
#[tauri::command]
fn cancel_job(service: State<Service>, id: String) -> Result<(), String> {
	let mut service = service.lock().unwrap_or_else(|e| e.into_inner());
	let AdminService {
		jobs, processes, ..
	} = &mut *service;
	jobs.cancel(&id, processes).map_err(error)
}

#[tauri::command]
fn save_card(
	service: State<Service>,
	id: Option<String>,
	draft: crate::board::Draft,
) -> Result<crate::board::Card, String> {
	service
		.lock()
		.unwrap_or_else(|e| e.into_inner())
		.board
		.save(id.as_deref(), draft)
		.map_err(error)
}
#[tauri::command]
fn delete_card(service: State<Service>, id: String) -> Result<(), String> {
	service
		.lock()
		.unwrap_or_else(|e| e.into_inner())
		.board
		.delete(&id)
		.map_err(error)
}
#[tauri::command]
fn move_card(
	service: State<Service>,
	id: String,
	column: crate::board::Column,
	before: Option<String>,
) -> Result<(), String> {
	service
		.lock()
		.unwrap_or_else(|e| e.into_inner())
		.board
		.move_card(&id, column, before.as_deref())
		.map_err(error)
}
#[tauri::command]
fn link_card_job(service: State<Service>, id: String, job: String) -> Result<(), String> {
	let mut service = service.lock().unwrap_or_else(|e| e.into_inner());
	if !service.jobs.runs.iter().any(|r| r.id == job) {
		return Err("Job no longer exists".into());
	}
	service.board.link(&id, &job).map_err(error)
}
#[tauri::command]
fn queue_card_job(
	service: State<Service>,
	id: String,
	kind: crate::jobs::Kind,
	package: String,
	profile: String,
) -> Result<crate::jobs::Run, String> {
	let mut service = service.lock().unwrap_or_else(|e| e.into_inner());
	if !service
		.board
		.cards
		.iter()
		.any(|c| c.id == id && c.jobs.len() < 100)
		|| service.board.storage_error.is_some()
	{
		return Err("Card unavailable or link limit reached".into());
	}
	let workspace = service.workspace.clone();
	let job = service
		.jobs
		.queue(&workspace, kind, package, profile)
		.map_err(error)?;
	if let Err(e) = service.board.link(&id, &job.id) {
		let AdminService {
			jobs, processes, ..
		} = &mut *service;
		let _ = jobs.cancel(&job.id, processes);
		return Err(error(e));
	}
	Ok(job)
}
#[tauri::command]
fn preview_project(
	service: State<Service>,
	input: crate::builder::Input,
) -> Result<crate::builder::Preview, String> {
	service
		.lock()
		.unwrap_or_else(|e| e.into_inner())
		.builder
		.preview(input)
		.map_err(error)
}
#[tauri::command]
fn generate_project(
	service: State<Service>,
	id: String,
) -> Result<crate::builder::Generation, String> {
	let mut service = service.lock().unwrap_or_else(|e| e.into_inner());
	let AdminService { builder, jobs, .. } = &mut *service;
	builder.generate(&id, jobs).map_err(error)
}
#[derive(serde::Serialize)]
struct ExportResult {
	path: String,
	count: usize,
}
#[tauri::command]
fn export_logs(service: State<Service>, filter: LogFilter) -> Result<ExportResult, String> {
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
fn open_source(service: State<Service>, cursor: u64) -> Result<(), String> {
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
pub fn run() {
	let _logging =
		engine_logger::init_application("admin-dashboard").expect("initialize admin diagnostics");
	let logs = engine_logger::log_directory();
	let data = std::env::var_os("ENGINE_ADMIN_DATA_DIR")
		.map(PathBuf::from)
		.unwrap_or_else(|| logs.parent().unwrap_or(&logs).join("admin"));
	let settings = std::fs::read(data.join("settings.json"))
		.ok()
		.and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
	let workspace = std::env::var_os("ENGINE_WORKSPACE_DIR")
		.map(PathBuf::from)
		.or_else(|| {
			settings
				.as_ref()
				.and_then(|s| s["workspace"].as_str().map(PathBuf::from))
		})
		.unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.."));
	let logs = std::env::var_os("ENGINE_LOG_DIR")
		.map(PathBuf::from)
		.or_else(|| {
			settings
				.as_ref()
				.and_then(|s| s["logs"].as_str().map(PathBuf::from))
		})
		.unwrap_or(logs);
	let service = Arc::new(Mutex::new(AdminService::new(workspace, logs, data)));
	crate::start_worker(&service);
	let owned = service.clone();
	let app = tauri::Builder::default()
		.manage(service)
		.invoke_handler(tauri::generate_handler![
			dashboard_snapshot,
			queue_job,
			save_card,
			delete_card,
			move_card,
			link_card_job,
			queue_card_job,
			preview_project,
			generate_project,
			cancel_job,
			log_page,
			configure_workspace,
			start_application,
			stop_application,
			restart_application,
			export_logs,
			open_source
		])
		.build(tauri::generate_context!())
		.expect("build admin dashboard");
	app.run(move |_, event| {
		if matches!(event, tauri::RunEvent::Exit) {
			let mut service = owned.lock().unwrap_or_else(|e| e.into_inner());
			service.jobs.interrupt();
			service.processes.shutdown();
			service.tick();
		}
	});
}
