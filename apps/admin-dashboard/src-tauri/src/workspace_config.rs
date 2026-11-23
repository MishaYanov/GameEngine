use crate::{AdminService, collector::Collector, health::Registry, jobs, processes, save_json};
use std::{
	fs, io,
	path::{Path, PathBuf},
};

impl AdminService {
	pub fn configure(&mut self, workspace: &str, logs: &str) -> io::Result<()> {
		if self.builder.has_preparation() {
			return Err(io::Error::other(
				"Finish project preparation before changing workspace",
			));
		}
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
			.history_ref()
			.iter()
			.any(|r| r.state == processes::ProcessState::Running)
		{
			return Err(io::Error::other(
				"Stop managed applications before changing workspace",
			));
		}
		let workspace = if workspace.trim().is_empty() {
			None
		} else {
			Some(resolve_workspace(Path::new(workspace))?)
		};
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
		self.builder.clear_previews();
		save_json(
			&self.data.join("settings.json"),
			&serde_json::json!({"workspace":workspace,"logs":logs}),
		)?;
		self.tick();
		Ok(())
	}
}

pub(crate) fn resolve_workspace(workspace: &Path) -> io::Result<PathBuf> {
	let workspace = workspace.canonicalize()?;
	if !workspace.join("Cargo.toml").is_file() {
		return Err(io::Error::other("Workspace must contain Cargo.toml"));
	}
	Ok(workspace)
}
