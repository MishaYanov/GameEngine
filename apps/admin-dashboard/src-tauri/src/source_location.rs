use crate::{AdminService, relative_to};
use std::{io, path::PathBuf};

impl AdminService {
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
}
