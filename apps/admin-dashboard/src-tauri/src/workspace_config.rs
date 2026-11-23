use crate::{AdminService, collector::Collector, health::Registry, jobs, processes, save_json};
use serde::{Deserialize, Serialize};
use std::{
	fs, io,
	path::{Path, PathBuf},
};

#[derive(Default, Deserialize, Serialize)]
pub(crate) struct WorkspaceSettings {
	pub workspace: Option<PathBuf>,
	pub trusted_workspace: Option<PathBuf>,
	pub logs: Option<PathBuf>,
}

impl WorkspaceSettings {
	pub(crate) fn trusts(&self, workspace: &Path) -> bool {
		self.trusted_workspace.as_deref() == Some(workspace)
	}
}

impl AdminService {
	pub(crate) fn trusted_workspace(&self) -> io::Result<PathBuf> {
		let workspace = self
			.workspace
			.as_ref()
			.filter(|_| self.workspace_trusted)
			.ok_or_else(|| {
				io::Error::other(
					"Review and trust a source checkout under Overview > Workspace settings.",
				)
			})?;
		ensure_reviewed_path(workspace)?;
		Ok(workspace.clone())
	}

	pub(crate) fn configure(
		&mut self,
		reviewed_workspace: Option<PathBuf>,
		logs: &str,
	) -> io::Result<()> {
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
		if let Some(workspace) = &reviewed_workspace {
			ensure_reviewed_path(workspace)?;
		}
		let logs = PathBuf::from(logs);
		fs::create_dir_all(&logs)?;
		let logs = logs.canonicalize()?;
		let checkpoint = self.data.join("logs.json");
		if checkpoint.exists() {
			fs::remove_file(checkpoint)?;
		}
		save_json(
			&self.data.join("settings.json"),
			&WorkspaceSettings {
				workspace: reviewed_workspace.clone(),
				trusted_workspace: reviewed_workspace.clone(),
				logs: Some(logs.clone()),
			},
		)?;
		self.collector = Collector::new(logs, &self.data);
		self.registry = Registry::new();
		self.workspace_trusted = reviewed_workspace.is_some();
		self.workspace = reviewed_workspace;
		self.builder.clear_previews();
		self.tick();
		Ok(())
	}
}

pub(crate) fn resolve_workspace(workspace: &Path) -> io::Result<PathBuf> {
	let workspace = workspace.canonicalize()?;
	let manifest = workspace.join("Cargo.toml");
	if !manifest.is_file() || manifest.canonicalize()?.parent() != Some(workspace.as_path()) {
		return Err(io::Error::other(
			"Workspace must contain its own Cargo.toml",
		));
	}
	Ok(workspace)
}

fn ensure_reviewed_path(workspace: &Path) -> io::Result<()> {
	if resolve_workspace(workspace)? != workspace {
		return Err(io::Error::other(
			"Source checkout changed. Review its location again.",
		));
	}
	Ok(())
}
