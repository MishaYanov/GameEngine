pub mod board;
pub mod builder;
pub mod collector;
#[cfg(feature = "desktop")]
pub mod desktop;
pub mod health;
#[cfg(windows)]
mod job;
pub mod jobs;
pub mod processes;
use collector::{Collector, LogFilter};
use health::Registry;
use processes::{LaunchTarget, ProcessRun, Processes};
use serde::Serialize;
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
pub fn comparable_path(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if let Some(unc) = text.strip_prefix("\\\\?\\UNC\\") {
            return PathBuf::from(format!("\\\\{unc}"));
        }
        if let Some(normal) = text.strip_prefix("\\\\?\\") {
            return PathBuf::from(normal);
        }
    }
    path.to_owned()
}
pub fn relative_to(path: &Path, root: &Path) -> Option<PathBuf> {
    comparable_path(path)
        .strip_prefix(comparable_path(root))
        .ok()
        .map(Path::to_owned)
}
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn save_json(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("Missing data directory"))?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension("tmp");
    let mut file = fs::File::create(&temporary)?;
    serde_json::to_writer(&mut file, value)?;
    file.flush()?;
    drop(file);
    fs::rename(temporary, path)
}
#[derive(Serialize)]
pub struct DashboardSnapshot {
    pub workspace: String,
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
    pub workspace: PathBuf,
    pub data: PathBuf,
}
impl AdminService {
    pub fn new(workspace: PathBuf, logs: PathBuf, data: PathBuf) -> Self {
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
            workspace,
            data,
        };
        for run in service.processes.history() {
            if run.state == processes::ProcessState::Interrupted
                && let Some(session) = run.session_id
            {
                service.registry.stopped(&session, now_ms(), true);
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
            .history()
            .into_iter()
            .filter(|run| run.state != processes::ProcessState::Running)
        {
            if let Some(session) = run.session_id {
                self.registry.stopped(
                    &session,
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
        if let Err(error) = save_json(&self.data.join("components.json"), &self.registry) {
            self.collector.health.checkpoint_error = Some(error.to_string());
        }
    }
    pub fn snapshot(&self) -> DashboardSnapshot {
        DashboardSnapshot {
            workspace: self.workspace.to_string_lossy().into_owned(),
            log_directory: self.collector.directory.to_string_lossy().into_owned(),
            data_directory: self.data.to_string_lossy().into_owned(),
            collector: self.collector.health.clone(),
            components: self.registry.snapshot(now_ms()),
            processes: self
                .processes
                .history()
                .into_iter()
                .filter(|r| !r.application.starts_with("job-"))
                .collect(),
            jobs: self.jobs.runs.clone(),
            cards: self.board.cards.clone(),
            board_storage_error: self.board.storage_error.clone(),
            projects: self.builder.runs.clone(),
            project_storage_error: self.builder.storage_error.clone(),
            job_storage_error: self.jobs.storage_error.clone(),
            targets: Processes::targets(&self.workspace),
            process_storage_error: self.processes.persistence_error.clone(),
            process_output_drops: self
                .processes
                .output_drops
                .load(std::sync::atomic::Ordering::Relaxed),
        }
    }
    pub fn configure(&mut self, workspace: &str, logs: &str) -> io::Result<()> {
        if self
            .jobs
            .runs
            .iter()
            .any(|r| matches!(r.state, jobs::Status::Queued | jobs::Status::Running))
        {
            return Err(io::Error::other(
                "Finish or cancel jobs before changing workspace",
            ));
        }
        if self
            .processes
            .history()
            .iter()
            .any(|r| r.state == processes::ProcessState::Running)
        {
            return Err(io::Error::other(
                "Stop managed applications before changing workspace",
            ));
        }
        let workspace = PathBuf::from(workspace).canonicalize()?;
        if !workspace.join("Cargo.toml").is_file() {
            return Err(io::Error::other("Workspace must contain Cargo.toml"));
        }
        let logs = PathBuf::from(logs);
        fs::create_dir_all(&logs)?;
        let logs = logs.canonicalize()?;
        let checkpoint = self.data.join("logs.json");
        if checkpoint.exists() {
            fs::remove_file(checkpoint)?;
        }
        self.collector = Collector::new(logs.clone(), &self.data);
        self.registry = Registry::new();
        self.workspace = workspace.clone();
        save_json(
            &self.data.join("settings.json"),
            &serde_json::json!({"workspace":workspace,"logs":logs}),
        )?;
        self.tick();
        Ok(())
    }
    pub fn source(&self, cursor: u64) -> io::Result<(PathBuf, u32)> {
        let entry = self
            .collector
            .find(cursor)
            .ok_or_else(|| io::Error::other("Record is no longer retained"))?;
        let file = entry
            .file
            .as_ref()
            .ok_or_else(|| io::Error::other("Record has no source location"))?;
        let job_workspace = entry
            .correlation
            .operation_id
            .as_ref()
            .and_then(|id| self.jobs.runs.iter().find(|r| &r.id == id))
            .map(|r| r.workspace.as_path());
        let project = entry.correlation.operation_id.as_ref().and_then(|id| {
            self.builder
                .runs
                .iter()
                .find(|r| &r.job_id == id && r.state == "SUCCEEDED")
        });
        let root = project
            .map(|r| r.destination.as_path())
            .or(job_workspace)
            .unwrap_or(&self.workspace);
        let mut source = PathBuf::from(file);
        if let Some(project) = project
            && let Some(relative) = relative_to(&source, &project.staging)
        {
            source = project.destination.join(relative);
        }

        let source = if source.is_absolute() {
            source
        } else {
            root.join(source)
        }
        .canonicalize()?;
        let workspace = root.canonicalize()?;
        if !source.starts_with(workspace) || !source.is_file() {
            return Err(io::Error::other(
                "Source must be a file inside the selected workspace",
            ));
        }
        Ok((source, entry.line.unwrap_or(1)))
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
#[cfg(test)]
#[path = "tests_admin.rs"]
mod tests;
