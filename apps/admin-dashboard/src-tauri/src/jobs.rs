use crate::{
	now_ms,
	processes::{ProcessState, Processes},
	save_json,
};
use engine_logger::{Context, Correlation, Level, LogEntry};
use serde::{Deserialize, Serialize};
use std::{
	fs::File,
	io,
	io::Read,
	path::{Path, PathBuf},
	process::Command,
};
mod cargo_output;

use cargo_output::CargoOutput;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum Kind {
	Build,
	Test,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum Status {
	Queued,
	Running,
	Succeeded,
	Failed,
	Cancelled,
	Interrupted,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Run {
	pub id: String,
	pub kind: Kind,
	pub package: String,
	pub profile: String,
	pub workspace: PathBuf,
	pub state: Status,
	pub queued_ms: u64,
	pub started_ms: Option<u64>,
	pub ended_ms: Option<u64>,
	pub process_id: Option<String>,
	pub exit_code: Option<i32>,
	pub toolchain: Option<String>,
	pub artifacts: Vec<String>,
	#[serde(default)]
	pub offline: bool,
	pub detail: Option<String>,
}

impl Run {
	fn failure(&self, phase: &'static str, io_kind: Option<io::ErrorKind>) {
		let mut correlation = Correlation {
			operation_id: Some(self.id.clone()),
			..Default::default()
		};
		match self.kind {
			Kind::Build => correlation.build_id = Some(self.id.clone()),
			Kind::Test => correlation.test_run_id = Some(self.id.clone()),
		}
		engine_logger::event(
			Level::Error,
			"admin::jobs",
			format_args!(
				"Cargo job failed: phase={phase}, exit_code={:?}, io_kind={io_kind:?}",
				self.exit_code
			),
			Context {
				correlation,
				..Default::default()
			},
		);
	}
}
pub const PACKAGES: &[&str] = &[
	"game",
	"app",
	"compiler",
	"runtime",
	"renderer",
	"bridge",
	"engine-logger",
	"engine-admin",
	"engine-core",
	"engine-headless",
];
pub struct Jobs {
	pub runs: Vec<Run>,
	path: PathBuf,
	pub storage_error: Option<String>,
	sequence: u64,
}
impl Jobs {
	pub fn new(data: &Path) -> Self {
		let path = data.join("jobs.json");
		let result = File::open(&path).and_then(|file| {
			serde_json::from_reader::<_, Vec<Run>>(file.take(4 * 1024 * 1024))
				.map_err(io::Error::other)
		});
		let (mut runs, storage_error) = match result {
			Ok(r) => (r, None),
			Err(e) if e.kind() == io::ErrorKind::NotFound => (vec![], None),
			Err(e) => (vec![], Some(e.to_string())),
		};
		runs.truncate(200);
		for run in &mut runs {
			if matches!(run.state, Status::Running | Status::Queued) {
				run.state = Status::Interrupted;
				run.ended_ms = Some(now_ms());
				run.detail = Some("Dashboard restarted; job was not resumed".into());
			}
		}
		let mut jobs = Self {
			runs,
			path,
			storage_error,
			sequence: 0,
		};
		jobs.persist();
		jobs
	}
	fn persist(&mut self) {
		self.storage_error = save_json(&self.path, &self.runs)
			.err()
			.map(|e| e.to_string());
	}
	pub fn queue(
		&mut self,
		workspace: &Path,
		kind: Kind,
		package: String,
		profile: String,
	) -> io::Result<Run> {
		if !PACKAGES.contains(&package.as_str()) || !["dev", "release"].contains(&profile.as_str())
		{
			return Err(io::Error::other("Unsupported Cargo package or profile"));
		}
		if self
			.runs
			.iter()
			.filter(|r| matches!(r.state, Status::Queued | Status::Running))
			.count() >= 50
		{
			return Err(io::Error::other("Job queue is full (50 jobs)"));
		}
		self.sequence += 1;
		let run = Run {
			id: format!("job-{}-{}-{}", std::process::id(), now_ms(), self.sequence),
			kind,
			package,
			profile,
			workspace: workspace.to_owned(),
			state: Status::Queued,
			queued_ms: now_ms(),
			started_ms: None,
			ended_ms: None,
			process_id: None,
			exit_code: None,
			toolchain: None,
			artifacts: vec![],
			offline: false,
			detail: None,
		};
		self.runs.insert(0, run.clone());
		self.runs.truncate(200);
		self.persist();
		Ok(run)
	}
	pub fn set_offline(&mut self, id: &str, offline: bool) {
		if let Some(run) = self.runs.iter_mut().find(|r| r.id == id) {
			run.offline = offline;
		}
		self.persist();
	}
	pub fn interrupt(&mut self) {
		for run in &mut self.runs {
			if matches!(run.state, Status::Queued | Status::Running) {
				run.state = Status::Interrupted;
				run.ended_ms = Some(now_ms());
				run.detail = Some("Dashboard closed before completion".into());
			}
		}
		self.persist();
	}
	pub fn cancel(&mut self, id: &str, processes: &mut Processes) -> io::Result<()> {
		let run = self
			.runs
			.iter_mut()
			.find(|r| r.id == id)
			.ok_or_else(|| io::Error::other("Unknown job"))?;
		if !matches!(run.state, Status::Queued | Status::Running) {
			return Err(io::Error::other("Job has already finished"));
		}
		if run.state == Status::Running {
			processes.stop(&run.id)?;
		}
		run.state = Status::Cancelled;
		run.ended_ms = Some(now_ms());
		self.persist();
		Ok(())
	}
	pub fn tick(&mut self, processes: &mut Processes, logs: &Path) {
		let history = processes.history_ref();
		let mut changed = false;
		for run in &mut self.runs {
			if run.state == Status::Running
				&& let Some(process) = history
					.iter()
					.find(|p| Some(&p.id) == run.process_id.as_ref())
				&& process.state != ProcessState::Running
				&& processes.output_complete(&process.id)
			{
				run.state = match process.state {
					ProcessState::Succeeded => Status::Succeeded,
					ProcessState::Stopped => Status::Cancelled,
					ProcessState::Interrupted => Status::Interrupted,
					_ => Status::Failed,
				};
				run.exit_code = process.exit_code;
				run.ended_ms = process.ended_ms;
				if run.state == Status::Failed {
					run.failure("exit", None);
				}
				changed = true;
			}
		}
		if !self.runs.iter().any(|r| r.state == Status::Running)
			&& let Some(run) = self
				.runs
				.iter_mut()
				.rev()
				.find(|r| r.state == Status::Queued)
		{
			let mut command = Command::new("cargo");
			command.args([
				if run.kind == Kind::Build {
					"build"
				} else {
					"test"
				},
				"-p",
				&run.package,
				"--profile",
				&run.profile,
				"--message-format=json",
			]);
			if run.offline {
				command.arg("--offline");
			}
			let mut version = Command::new("cargo");
			version.arg("--version");
			crate::processes::hidden(&mut version);
			run.toolchain = version
				.output()
				.ok()
				.filter(|o| o.status.success())
				.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());
			run.started_ms = Some(now_ms());
			match processes.start_command(&run.id, command, &run.workspace, logs) {
				Ok(process) => {
					run.process_id = Some(process.id);
					run.state = Status::Running;
				}
				Err(e) => {
					run.state = Status::Failed;
					run.detail = Some(e.to_string());
					run.ended_ms = Some(now_ms());
					run.failure("spawn", Some(e.kind()));
				}
			}
			changed = true;
		}
		if changed {
			self.persist();
		}
	}
	pub fn relocate_artifacts(&mut self, id: &str, from: &Path, to: &Path) {
		let mut changed = false;
		if let Some(run) = self.runs.iter_mut().find(|r| r.id == id) {
			for artifact in &mut run.artifacts {
				if let Some(relative) = crate::relative_to(Path::new(artifact), from) {
					*artifact = to.join(relative).to_string_lossy().into_owned();
					changed = true;
				}
			}
		}
		if changed {
			self.persist();
		}
	}
	pub fn enrich(&mut self, entry: &mut LogEntry) {
		let Some(run) = self
			.runs
			.iter_mut()
			.find(|r| Some(&r.id) == entry.correlation.operation_id.as_ref())
		else {
			return;
		};
		if run.kind == Kind::Build {
			entry.correlation.build_id = Some(run.id.clone());
		} else {
			entry.correlation.test_run_id = Some(run.id.clone());
		}
		match CargoOutput::parse(&entry.message, &run.package) {
			Some(CargoOutput::Diagnostic {
				level,
				message,
				source,
			}) => {
				entry.component = "cargo-diagnostic".into();
				entry.level = level;
				entry.message = message;
				if let Some(source) = source {
					entry.file = source.file;
					entry.line = source.line;
				}
			}
			Some(CargoOutput::Artifact { filenames }) => {
				for filename in filenames {
					if run.artifacts.len() < 128 && !run.artifacts.contains(&filename) {
						run.artifacts.push(filename);
					}
				}
				self.persist();
			}
			None => {}
		}
	}
}
#[cfg(test)]
#[path = "../tests/unit/tests_jobs.rs"]
mod tests;
