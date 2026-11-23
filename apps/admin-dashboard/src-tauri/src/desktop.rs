use crate::{
	AdminService, DashboardSnapshot,
	processes::ProcessRun,
	workspace_config::{WorkspaceSettings, resolve_workspace},
};
use std::{
	path::{Path, PathBuf},
	sync::{Arc, Mutex},
};
use tauri::{State, Url, WebviewWindow};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
type Service = Arc<Mutex<AdminService>>;
static WORKSPACE_REVIEW: Mutex<()> = Mutex::new(());

mod logs;
mod projects;

fn is_trusted_origin(url: &Url) -> bool {
	let packaged = url.port().is_none()
		&& matches!(
			(url.scheme(), url.host_str()),
			("tauri", Some("localhost")) | ("http", Some("tauri.localhost"))
		);
	let development = cfg!(debug_assertions)
		&& url.scheme() == "http"
		&& url.host_str() == Some("127.0.0.1")
		&& url.port() == Some(4301);
	packaged || development
}

fn trusted_window(window: &WebviewWindow) -> Result<(), String> {
	if window.label() != "main" || !is_trusted_origin(&window.url().map_err(error)?) {
		return Err("Dashboard commands require the trusted main application window".into());
	}
	Ok(())
}

fn error(error: impl ToString) -> String {
	error.to_string()
}

#[tauri::command]
fn dashboard_snapshot(
	window: WebviewWindow,
	service: State<Service>,
) -> Result<DashboardSnapshot, String> {
	trusted_window(&window)?;
	Ok(service.lock().unwrap_or_else(|e| e.into_inner()).snapshot())
}

#[tauri::command]
async fn configure_workspace(
	window: WebviewWindow,
	service: State<'_, Service>,
	workspace: String,
	logs: String,
) -> Result<(), String> {
	trusted_window(&window)?;
	let service = Arc::clone(service.inner());
	tauri::async_runtime::spawn_blocking(move || {
		let _review = WORKSPACE_REVIEW
			.try_lock()
			.map_err(|_| "A workspace settings review is already in progress".to_owned())?;
		let workspace = workspace.trim();
		let reviewed = (!workspace.is_empty())
			.then(|| resolve_workspace(Path::new(workspace)))
			.transpose()
			.map_err(error)?;
		if let Some(path) = reviewed.as_deref() {
			let already_trusted = {
				let service = service.lock().unwrap_or_else(|e| e.into_inner());
				service.workspace_trusted && service.workspace.as_deref() == Some(path)
			};
			if !already_trusted && !confirm_workspace_trust(&window, path) {
				return Err("Workspace trust review canceled".into());
			}
		}
		trusted_window(&window)?;
		let mut service = service.lock().unwrap_or_else(|e| e.into_inner());
		service.configure(reviewed, &logs).map_err(error)
	})
	.await
	.map_err(|_| "Workspace settings review worker failed".to_owned())?
}

fn confirm_workspace_trust(window: &WebviewWindow, path: &Path) -> bool {
	window
		.dialog()
		.message(format!(
			"Trust this source checkout? Builds, tests, and application launches can execute its code.\n\n{}",
			crate::comparable_path(path).display()
		))
		.title("Review workspace trust")
		.parent(window)
		.kind(MessageDialogKind::Warning)
		.buttons(MessageDialogButtons::OkCancelCustom("Trust checkout".into(), "Cancel".into()))
		.blocking_show()
}
#[tauri::command]
fn start_application(
	window: WebviewWindow,
	service: State<Service>,
	id: String,
) -> Result<ProcessRun, String> {
	trusted_window(&window)?;
	let mut service = service.lock().unwrap_or_else(|e| e.into_inner());
	let workspace = service.trusted_workspace().map_err(error)?;
	let logs = service.collector.directory.clone();
	service
		.processes
		.start(&workspace, &logs, &id)
		.map_err(error)
}
#[tauri::command]
fn stop_application(
	window: WebviewWindow,
	service: State<Service>,
	id: String,
) -> Result<(), String> {
	trusted_window(&window)?;
	service
		.lock()
		.unwrap_or_else(|e| e.into_inner())
		.processes
		.stop(&id)
		.map_err(error)
}
#[tauri::command]
fn restart_application(
	window: WebviewWindow,
	service: State<Service>,
	id: String,
) -> Result<ProcessRun, String> {
	trusted_window(&window)?;
	let mut service = service.lock().unwrap_or_else(|e| e.into_inner());
	let workspace = service.trusted_workspace().map_err(error)?;
	service.processes.stop(&id).map_err(error)?;
	let logs = service.collector.directory.clone();
	service
		.processes
		.start(&workspace, &logs, &id)
		.map_err(error)
}
#[tauri::command]
fn queue_job(
	window: WebviewWindow,
	service: State<Service>,
	kind: crate::jobs::Kind,
	package: String,
	profile: String,
) -> Result<crate::jobs::Run, String> {
	trusted_window(&window)?;
	let mut service = service.lock().unwrap_or_else(|e| e.into_inner());
	let workspace = service.trusted_workspace().map_err(error)?;
	service
		.jobs
		.queue(&workspace, kind, package, profile)
		.map_err(error)
}
#[tauri::command]
fn cancel_job(window: WebviewWindow, service: State<Service>, id: String) -> Result<(), String> {
	trusted_window(&window)?;
	let mut service = service.lock().unwrap_or_else(|e| e.into_inner());
	let AdminService {
		jobs, processes, ..
	} = &mut *service;
	jobs.cancel(&id, processes).map_err(error)
}

#[tauri::command]
fn save_card(
	window: WebviewWindow,
	service: State<Service>,
	id: Option<String>,
	draft: crate::board::Draft,
) -> Result<crate::board::Card, String> {
	trusted_window(&window)?;
	service
		.lock()
		.unwrap_or_else(|e| e.into_inner())
		.board
		.save(id.as_deref(), draft)
		.map_err(error)
}
#[tauri::command]
fn delete_card(window: WebviewWindow, service: State<Service>, id: String) -> Result<(), String> {
	trusted_window(&window)?;
	service
		.lock()
		.unwrap_or_else(|e| e.into_inner())
		.board
		.delete(&id)
		.map_err(error)
}
#[tauri::command]
fn move_card(
	window: WebviewWindow,
	service: State<Service>,
	id: String,
	column: crate::board::Column,
	before: Option<String>,
) -> Result<(), String> {
	trusted_window(&window)?;
	service
		.lock()
		.unwrap_or_else(|e| e.into_inner())
		.board
		.move_card(&id, column, before.as_deref())
		.map_err(error)
}
#[tauri::command]
fn link_card_job(
	window: WebviewWindow,
	service: State<Service>,
	id: String,
	job: String,
) -> Result<(), String> {
	trusted_window(&window)?;
	let mut service = service.lock().unwrap_or_else(|e| e.into_inner());
	if !service.jobs.runs.iter().any(|r| r.id == job) {
		return Err("Job no longer exists".into());
	}
	service.board.link(&id, &job).map_err(error)
}
#[tauri::command]
fn queue_card_job(
	window: WebviewWindow,
	service: State<Service>,
	id: String,
	kind: crate::jobs::Kind,
	package: String,
	profile: String,
) -> Result<crate::jobs::Run, String> {
	trusted_window(&window)?;
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
	let workspace = service.trusted_workspace().map_err(error)?;
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
pub fn run() {
	let _logging =
		engine_logger::init_application("admin-dashboard").expect("initialize admin diagnostics");
	let logs = engine_logger::log_directory();
	let data = std::env::var_os("ENGINE_ADMIN_DATA_DIR")
		.map(PathBuf::from)
		.unwrap_or_else(|| logs.parent().unwrap_or(&logs).join("admin"));
	let settings = std::fs::read(data.join("settings.json"))
		.ok()
		.and_then(|bytes| serde_json::from_slice::<WorkspaceSettings>(&bytes).ok())
		.unwrap_or_default();
	let workspace = std::env::var_os("ENGINE_WORKSPACE_DIR")
		.map(PathBuf::from)
		.and_then(|path| resolve_workspace(&path).ok())
		.or_else(|| {
			settings
				.workspace
				.as_deref()
				.and_then(|path| resolve_workspace(path).ok())
		});
	#[cfg(not(feature = "custom-protocol"))]
	let workspace = workspace.or_else(|| {
		resolve_workspace(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")).ok()
	});
	let workspace_trusted = workspace
		.as_deref()
		.is_some_and(|path| settings.trusts(path));
	let logs = std::env::var_os("ENGINE_LOG_DIR")
		.map(PathBuf::from)
		.or(settings.logs)
		.unwrap_or(logs);
	let mut admin = AdminService::new(workspace, logs, data);
	admin.workspace_trusted = workspace_trusted;
	let service = Arc::new(Mutex::new(admin));
	crate::start_worker(&service);
	let owned = service.clone();
	let app = tauri::Builder::default()
		.plugin(tauri_plugin_dialog::init())
		.manage(service)
		.invoke_handler(tauri::generate_handler![
			logs::resource_metrics,
			dashboard_snapshot,
			queue_job,
			save_card,
			delete_card,
			move_card,
			link_card_job,
			queue_card_job,
			projects::preview_project,
			projects::generate_project,
			cancel_job,
			logs::log_page,
			configure_workspace,
			start_application,
			stop_application,
			restart_application,
			logs::export_logs,
			logs::open_source
		])
		.build(tauri::generate_context!())
		.expect("build admin dashboard");
	engine_logger::info!(target: "admin", "Admin dashboard started");
	app.run(move |_, event| {
		if matches!(event, tauri::RunEvent::Exit) {
			let mut service = owned.lock().unwrap_or_else(|e| e.into_inner());
			service.builder.clear_previews();
			service.jobs.interrupt();
			service.processes.shutdown();
			service.tick();
		}
	});
	engine_logger::info!(target: "admin", "Admin dashboard stopped");
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn dashboard_uses_exact_application_origins() {
		for value in ["tauri://localhost/", "http://tauri.localhost/"] {
			assert!(is_trusted_origin(&Url::parse(value).unwrap()));
		}
		for value in ["https://web.example/", "http://tauri.localhost:4301/"] {
			assert!(!is_trusted_origin(&Url::parse(value).unwrap()));
		}
		let dev = Url::parse("http://127.0.0.1:4301/").unwrap();
		assert_eq!(is_trusted_origin(&dev), cfg!(debug_assertions));
	}
}
