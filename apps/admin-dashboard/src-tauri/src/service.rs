use crate::{
	board, builder,
	collector::{self, Collector, LogFilter},
	health::{self, Registry},
	jobs, now_ms,
	processes::{self, LaunchTarget, ProcessRun, Processes},
};
use serde::Serialize;
use std::{
	fs,
	io::{self, Read},
	path::PathBuf,
	sync::{Arc, Mutex},
};

#[derive(Serialize)]
pub struct DashboardSnapshot {
	pub workspace: String,
	pub workspace_trusted: bool,
	pub log_directory: String,
	pub data_directory: String,
	pub collector: collector::CollectorHealth,
	pub components: Vec<health::Component>,
	pub processes: Vec<ProcessRun>,
	pub targets: Vec<LaunchTarget>,
	pub process_storage_error: Option<String>,
	pub process_output_drops: u64,
	pub jobs: Vec<jobs::Run>,
	pub cards: Vec<board::Card>,
	pub board_storage_error: Option<String>,
	pub projects: Vec<builder::Generation>,
	pub project_storage_error: Option<String>,
	pub job_storage_error: Option<String>,
}
pub struct AdminService {
	pub collector: Collector,
	pub registry: Registry,
	pub processes: Processes,
	pub jobs: jobs::Jobs,
	pub board: board::Board,
	pub builder: builder::Builder,
	pub workspace: Option<PathBuf>,
	pub(crate) workspace_trusted: bool,
	pub data: PathBuf,
}
impl AdminService {
	pub fn new(workspace: impl Into<Option<PathBuf>>, logs: PathBuf, data: PathBuf) -> Self {
		let registry = fs::File::open(data.join("components.json"))
			.ok()
			.and_then(|file| serde_json::from_reader(file.take(512 * 1024)).ok())
			.unwrap_or_default();
		let mut service = Self {
			collector: Collector::new(logs, &data),
			registry,
			processes: Processes::new(&data),
			jobs: jobs::Jobs::new(&data),
			board: board::Board::new(&data),
			builder: builder::Builder::new(&data),
			workspace: workspace.into(),
			workspace_trusted: false,
			data,
		};
		for run in service.processes.history_ref() {
			if run.state == processes::ProcessState::Interrupted
				&& let Some(session) = &run.session_id
			{
				service.registry.stopped(session, now_ms(), true);
			}
		}
		service.tick();
		service
	}
	pub fn tick(&mut self) {
		for entry in self.collector.scan() {
			self.registry.accept(&entry);
			self.processes.bind_session(&entry);
		}
		for mut entry in self.processes.output() {
			self.jobs.enrich(&mut entry);
			self.collector.ingest(entry);
		}
		self.processes.poll();
		self.jobs
			.tick(&mut self.processes, &self.collector.directory);
		self.builder.tick(&mut self.jobs);
		for run in self
			.processes
			.history_ref()
			.iter()
			.filter(|run| run.state != processes::ProcessState::Running)
		{
			if let Some(session) = &run.session_id {
				self.registry.stopped(
					session,
					run.ended_ms.unwrap_or_else(now_ms),
					!run.intentional_stop
						&& matches!(
							run.state,
							processes::ProcessState::Failed | processes::ProcessState::Interrupted
						),
				);
			}
		}
		self.collector.persist();
		self.registry.persist(&self.data.join("components.json"));
	}
	pub fn snapshot(&self) -> DashboardSnapshot {
		let mut collector = self.collector.health.clone();
		if let Some(error) = self.registry.persistence_error() {
			collector.checkpoint_error = Some(error.into());
		}
		DashboardSnapshot {
			workspace: self
				.workspace
				.as_ref()
				.map(|path| path.to_string_lossy().into_owned())
				.unwrap_or_default(),
			workspace_trusted: self.workspace_trusted,
			log_directory: self.collector.directory.to_string_lossy().into_owned(),
			data_directory: self.data.to_string_lossy().into_owned(),
			collector,
			components: self.registry.snapshot(now_ms()),
			processes: self
				.processes
				.history_ref()
				.iter()
				.filter(|r| !r.application.starts_with("job-"))
				.cloned()
				.collect(),
			jobs: self.jobs.runs.clone(),
			cards: self.board.cards.clone(),
			board_storage_error: self.board.storage_error.clone(),
			projects: self.builder.runs.clone(),
			project_storage_error: self.builder.storage_error.clone(),
			job_storage_error: self.jobs.storage_error.clone(),
			targets: self
				.workspace
				.as_deref()
				.filter(|_| self.workspace_trusted)
				.map(Processes::targets)
				.unwrap_or_default(),
			process_storage_error: self.processes.persistence_error.clone(),
			process_output_drops: self
				.processes
				.output_drops
				.load(std::sync::atomic::Ordering::Relaxed),
		}
	}
	pub fn export(&self, filter: &LogFilter) -> io::Result<(PathBuf, usize)> {
		self.collector.export(&self.data.join("exports"), filter)
	}
}
pub fn start_worker(service: &Arc<Mutex<AdminService>>) {
	let service = Arc::downgrade(service);
	std::thread::spawn(move || {
		loop {
			std::thread::sleep(std::time::Duration::from_millis(500));
			let Some(service) = service.upgrade() else {
				break;
			};
			service.lock().unwrap_or_else(|e| e.into_inner()).tick();
		}
	});
}
