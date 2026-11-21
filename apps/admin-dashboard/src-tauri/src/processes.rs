use crate::{now_ms, save_json};
use engine_logger::LogEntry;
use serde::{Deserialize, Serialize};
use std::{
	collections::BTreeMap,
	fs::File,
	io::{self, Read},
	path::{Path, PathBuf},
	process::{Child, Command, Stdio},
	sync::mpsc::{self, Receiver, SyncSender},
};
mod output;

use output::Output;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum ProcessState {
	Running,
	Succeeded,
	Failed,
	Stopped,
	Interrupted,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessRun {
	pub id: String,
	pub application: String,
	pub pid: u32,
	pub session_id: Option<String>,
	pub started_ms: u64,
	pub ended_ms: Option<u64>,
	pub state: ProcessState,
	pub exit_code: Option<i32>,
	pub intentional_stop: bool,
}
#[derive(Clone, Serialize)]
pub struct LaunchTarget {
	pub id: String,
	pub label: String,
	pub executable: String,
	pub available: bool,
}
struct Active {
	#[cfg(windows)]
	job: crate::job::Job,
	child: Child,
	run_id: String,
	stopping: bool,
}
pub struct Processes {
	active: BTreeMap<String, Active>,
	history: Vec<ProcessRun>,
	checkpoint: PathBuf,
	sender: SyncSender<Output>,
	receiver: Receiver<Output>,
	drains: BTreeMap<String, std::sync::Arc<std::sync::atomic::AtomicUsize>>,
	sequence: u64,
	session: String,
	pub persistence_error: Option<String>,
	pub output_drops: std::sync::Arc<std::sync::atomic::AtomicU64>,
}
pub fn hidden(command: &mut Command) {
	#[cfg(windows)]
	{
		use std::os::windows::process::CommandExt;
		command.creation_flags(0x08000000);
	}
	#[cfg(unix)]
	{
		use std::os::unix::process::CommandExt;
		command.process_group(0);
	}
}
impl Processes {
	pub fn new(data: &Path) -> Self {
		let checkpoint = data.join("processes.json");
		let mut persistence_error = None;
		let mut history: Vec<ProcessRun> = match File::open(&checkpoint) {
			Ok(file) => match serde_json::from_reader(file.take(1024 * 1024)) {
				Ok(history) => history,
				Err(error) => {
					persistence_error = Some(error.to_string());
					vec![]
				}
			},
			Err(error) if error.kind() == io::ErrorKind::NotFound => vec![],
			Err(error) => {
				persistence_error = Some(error.to_string());
				vec![]
			}
		};
		history.truncate(200);
		for run in &mut history {
			if run.state == ProcessState::Running {
				run.state = ProcessState::Interrupted;
				run.ended_ms = Some(now_ms());
			}
		}
		let (sender, receiver) = mpsc::sync_channel(1024);
		let mut result = Self {
			active: BTreeMap::new(),
			history,
			checkpoint,
			sender,
			receiver,
			drains: BTreeMap::new(),
			sequence: 0,
			session: format!("managed-{}-{}", std::process::id(), now_ms()),
			persistence_error,
			output_drops: Default::default(),
		};
		result.persist();
		result
	}
	pub fn targets(workspace: &Path) -> Vec<LaunchTarget> {
		[
			("game", "Game", "game"),
			("editor-host", "Client host", "app"),
			("compiler", "Compiler", "compiler"),
			("vulkan-probe", "Vulkan probe", "examples/vulkan_probe"),
		]
		.into_iter()
		.map(|(id, label, file)| {
			let mut path = workspace.join("target/debug").join(file);
			if cfg!(windows) {
				path.set_extension("exe");
			}
			LaunchTarget {
				id: id.into(),
				label: label.into(),
				available: path.is_file(),
				executable: path.to_string_lossy().into_owned(),
			}
		})
		.collect()
	}
	pub fn start(&mut self, workspace: &Path, logs: &Path, id: &str) -> io::Result<ProcessRun> {
		let target = Self::targets(workspace)
			.into_iter()
			.find(|t| t.id == id)
			.ok_or_else(|| io::Error::other("Unknown application"))?;
		self.start_command(id, Command::new(target.executable), workspace, logs)
	}
	pub fn start_command(
		&mut self,
		id: &str,
		mut command: Command,
		workspace: &Path,
		logs: &Path,
	) -> io::Result<ProcessRun> {
		self.poll();
		if self.active.contains_key(id) {
			return Err(io::Error::other("Application is already running"));
		}
		command
			.current_dir(workspace)
			.env("ENGINE_LOG_DIR", logs)
			.env("ENGINE_LOG_CONSOLE", "false")
			.stdout(Stdio::piped())
			.stderr(Stdio::piped());
		hidden(&mut command);
		let mut child = command.spawn()?;
		#[cfg(windows)]
		let job = match crate::job::Job::attach(&child) {
			Ok(job) => job,
			Err(error) => {
				let _ = child.kill();
				let _ = child.wait();
				return Err(error);
			}
		};
		let run_id = format!("{id}-{}-{}", now_ms(), child.id());
		let pending = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
		self.drains
			.retain(|id, _| self.history.iter().any(|r| &r.id == id));
		self.drains.insert(run_id.clone(), pending.clone());
		self.start_output_readers(&mut child, id, &run_id, &pending);
		let run = ProcessRun {
			id: run_id.clone(),
			application: id.into(),
			pid: child.id(),
			session_id: None,
			started_ms: now_ms(),
			ended_ms: None,
			state: ProcessState::Running,
			exit_code: None,
			intentional_stop: false,
		};
		self.active.insert(
			id.into(),
			Active {
				#[cfg(windows)]
				job,
				child,
				run_id,
				stopping: false,
			},
		);
		self.history.insert(0, run.clone());
		self.history.truncate(200);
		self.persist();
		Ok(run)
	}
	pub fn stop(&mut self, id: &str) -> io::Result<()> {
		self.poll();
		let active = self
			.active
			.get_mut(id)
			.ok_or_else(|| io::Error::other("Application is not owned or not running"))?;
		#[cfg(windows)]
		{
			active.job.terminate()?;
		}
		#[cfg(unix)]
		{
			let result = Command::new("kill")
				.args(["-KILL", "--", &format!("-{}", active.child.id())])
				.status()?;
			if !result.success() {
				active.child.kill()?;
			}
		}
		active.stopping = true;
		if let Some(run) = self.history.iter_mut().find(|r| r.id == active.run_id) {
			run.intentional_stop = true;
		}
		let _ = active.child.wait();
		self.poll();
		self.persist();
		Ok(())
	}
	pub fn poll(&mut self) -> Vec<ProcessRun> {
		let mut finished = vec![];
		self.active
			.retain(|_, active| match active.child.try_wait() {
				Ok(Some(status)) => {
					if let Some(run) = self.history.iter_mut().find(|r| r.id == active.run_id) {
						run.ended_ms = Some(now_ms());
						run.exit_code = status.code();
						run.state = if active.stopping {
							ProcessState::Stopped
						} else if status.success() {
							ProcessState::Succeeded
						} else {
							ProcessState::Failed
						};
						finished.push(run.clone());
					}
					false
				}
				_ => true,
			});
		if !finished.is_empty() {
			self.persist();
		}
		finished
	}
	pub fn bind_session(&mut self, entry: &LogEntry) {
		if let Some(run) = self.history.iter_mut().find(|r| {
			r.application == entry.application
				&& r.pid == entry.process_id
				&& entry.timestamp_ms >= r.started_ms.saturating_sub(1000)
				&& r.ended_ms.is_none_or(|end| entry.timestamp_ms <= end)
		}) && run.session_id.is_none()
		{
			run.session_id = Some(entry.session_id.clone());
			self.persist();
		}
	}
	pub fn history(&self) -> Vec<ProcessRun> {
		self.history.clone()
	}
	pub fn history_ref(&self) -> &[ProcessRun] {
		&self.history
	}
	fn persist(&mut self) {
		match save_json(&self.checkpoint, &self.history) {
			Ok(()) => self.persistence_error = None,
			Err(error) => self.persistence_error = Some(error.to_string()),
		}
	}
	pub fn shutdown(&mut self) {
		let ids = self.active.keys().cloned().collect::<Vec<_>>();
		for id in ids {
			let _ = self.stop(&id);
		}
	}
}
impl Drop for Processes {
	fn drop(&mut self) {
		self.shutdown();
	}
}
