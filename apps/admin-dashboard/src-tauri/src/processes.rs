use crate::{now_ms, save_json};
use engine_logger::{Level, LogEntry};
use serde::{Deserialize, Serialize};
use std::{
	collections::BTreeMap,
	fs::File,
	io::{self, BufRead, Read},
	path::{Path, PathBuf},
	process::{Child, Command, Stdio},
	sync::mpsc::{self, Receiver, SyncSender},
};
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
struct Output {
	pending: std::sync::Arc<std::sync::atomic::AtomicUsize>,
	run_id: String,
	application: String,
	line: String,
	error: bool,
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
		for (stream, error) in [
			(
				child
					.stdout
					.take()
					.map(|s| Box::new(s) as Box<dyn Read + Send>),
				false,
			),
			(
				child
					.stderr
					.take()
					.map(|s| Box::new(s) as Box<dyn Read + Send>),
				true,
			),
		] {
			if let Some(stream) = stream {
				let pending = pending.clone();
				pending.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
				let sender = self.sender.clone();
				let run_id = run_id.clone();
				let application = id.to_owned();
				let drops = self.output_drops.clone();
				std::thread::spawn(move || {
					let mut reader = io::BufReader::new(stream);
					loop {
						let mut line = vec![];
						match reader.by_ref().take(8192).read_until(b'\n', &mut line) {
							Ok(0) | Err(_) => break,
							Ok(_) => {
								let line = String::from_utf8_lossy(&line).trim_end().to_owned();
								pending.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
								if sender
									.try_send(Output {
										pending: pending.clone(),
										run_id: run_id.clone(),
										application: application.clone(),
										line,
										error,
									})
									.is_err()
								{
									pending.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
									drops.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
								}
							}
						}
					}
					pending.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
				});
			}
		}
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
	pub fn output(&mut self) -> Vec<LogEntry> {
		let mut records = vec![];
		while let Ok(output) = self.receiver.try_recv() {
			output
				.pending
				.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
			self.sequence += 1;
			records.push(LogEntry {
				schema_version: engine_logger::SCHEMA_VERSION,
				session_id: self.session.clone(),
				sequence: self.sequence,
				timestamp_ms: now_ms(),
				application: output.application.clone(),
				component: "process-output".into(),
				process_id: self
					.history
					.iter()
					.find(|r| r.id == output.run_id)
					.map_or(0, |r| r.pid),
				thread: "admin-output-reader".into(),
				level: Level::Info,
				message: output.line,
				file: None,
				line: None,
				module: None,
				error: None,
				stack_trace: None,
				correlation: engine_logger::Correlation {
					operation_id: Some(if output.application.starts_with("job-") {
						output.application.clone()
					} else {
						output.run_id
					}),
					..Default::default()
				},
				attributes: [(
					"stream".into(),
					if output.error { "stderr" } else { "stdout" }.into(),
				)]
				.into(),
			});
		}
		records
	}
	pub fn output_complete(&self, id: &str) -> bool {
		self.drains
			.get(id)
			.is_none_or(|pending| pending.load(std::sync::atomic::Ordering::SeqCst) == 0)
	}
	pub fn history(&self) -> Vec<ProcessRun> {
		self.history.clone()
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
