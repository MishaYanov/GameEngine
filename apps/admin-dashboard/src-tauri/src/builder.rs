use crate::{
	jobs::{Jobs, Kind, Status},
	now_ms, save_json,
};
use serde::{Deserialize, Serialize};
use std::{
	collections::BTreeMap,
	fs,
	io::{self, Read},
	path::{Path, PathBuf},
	sync::{Arc, Weak},
};

mod template;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Input {
	pub name: String,
	pub source: String,
	pub destination: String,
	pub profile: String,
	pub target: String,
	#[serde(default = "offline_default")]
	pub offline: bool,
}
fn offline_default() -> bool {
	true
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileEntry {
	pub path: String,
	pub bytes: u64,
	pub fingerprint: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Preview {
	pub id: String,
	pub input: Input,
	pub files: Vec<FileEntry>,
	pub total_bytes: u64,
	pub excluded: Vec<String>,
	pub template_version: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Generation {
	pub id: String,
	pub name: String,
	pub destination: PathBuf,
	pub staging: PathBuf,
	pub source: PathBuf,
	pub state: String,
	pub job_id: String,
	pub detail: Option<String>,
	pub created_ms: u64,
	pub finished_ms: Option<u64>,
	pub revision: Option<String>,
	pub profile: String,
	pub target: String,
	#[serde(default)]
	pub files: Vec<FileEntry>,
}
pub struct Builder {
	pub runs: Vec<Generation>,
	pub storage_error: Option<String>,
	path: PathBuf,
	plans: BTreeMap<String, Preview>,
	sequence: u64,
	preview_preparations: BTreeMap<String, Weak<()>>,
	generation_preparation: Option<(String, Weak<()>)>,
	preparation_admission: Weak<()>,
}
// Owning the token reserves an operation. Dropping an abandoned task (including
// unwinding a worker panic) releases it without needing the service mutex.
struct Preparation {
	id: String,
	token: Arc<()>,
}
pub struct PreviewPreparation {
	reservation: Preparation,
	input: Input,
}
pub struct PreparedPreview {
	reservation: Preparation,
	preview: Preview,
}
pub struct GenerationPreparation {
	reservation: Preparation,
	plan: Preview,
}
pub struct PreparedGeneration {
	reservation: Preparation,
	run: Generation,
	offline: bool,
}
fn publish_new(source: &Path, destination: &Path) -> io::Result<()> {
	#[cfg(windows)]
	{
		use std::os::windows::ffi::OsStrExt;
		let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
		let destination: Vec<u16> = destination
			.as_os_str()
			.encode_wide()
			.chain(Some(0))
			.collect();
		// No replacement flag: a concurrent destination creator wins safely.
		if unsafe {
			windows_sys::Win32::Storage::FileSystem::MoveFileExW(
				source.as_ptr(),
				destination.as_ptr(),
				0,
			)
		} == 0
		{
			return Err(io::Error::last_os_error());
		}
		Ok(())
	}
	#[cfg(not(windows))]
	{
		let _ = (source, destination);
		Err(io::Error::other(
			"Project publication currently supports Windows only",
		))
	}
}

impl Builder {
	pub fn new(data: &Path) -> Self {
		let path = data.join("projects.json");
		let loaded = fs::File::open(&path).and_then(|f| {
			serde_json::from_reader::<_, Vec<Generation>>(f.take(32 * 1024 * 1024))
				.map_err(io::Error::other)
		});
		let (mut runs, storage_error) = match loaded {
			Ok(r) => (r, None),
			Err(e) if e.kind() == io::ErrorKind::NotFound => (vec![], None),
			Err(e) => (vec![], Some(e.to_string())),
		};
		runs.truncate(50);
		for run in &mut runs {
			if run.state == "VALIDATING" {
				run.state = "INTERRUPTED".into();
				run.detail = Some("Validation interrupted; staging retained for inspection".into());
				run.finished_ms = Some(now_ms());
			}
		}
		Self {
			runs,
			storage_error,
			path,
			plans: BTreeMap::new(),
			sequence: 0,
			preview_preparations: BTreeMap::new(),
			generation_preparation: None,
			preparation_admission: Weak::new(),
		}
	}
	fn persist(&mut self) {
		while self.runs.len() > 1
			&& serde_json::to_vec(&self.runs).is_ok_and(|bytes| bytes.len() > 32 * 1024 * 1024)
		{
			self.runs.pop();
		}
		self.storage_error = save_json(&self.path, &self.runs)
			.err()
			.map(|e| e.to_string());
	}
	pub fn has_preparation(&self) -> bool {
		self.preparation_admission.strong_count() != 0
	}
	pub fn clear_previews(&mut self) {
		// Invalidating publication does not release the memory allowance of a
		// worker that still owns its ticket and may still be reading files.
		self.plans.clear();
		self.preview_preparations.clear();
		self.generation_preparation = None;
	}
	pub fn begin_preview(&mut self, input: Input) -> io::Result<PreviewPreparation> {
		self.preview_preparations
			.retain(|_, token| token.strong_count() != 0);
		if self.has_preparation() {
			return Err(io::Error::other(
				"A project preparation is already in progress",
			));
		}
		self.sequence += 1;
		let id = format!(
			"project-{}-{}-{}",
			std::process::id(),
			now_ms(),
			self.sequence
		);
		let token = Arc::new(());
		self.preparation_admission = Arc::downgrade(&token);
		self.preview_preparations
			.insert(id.clone(), Arc::downgrade(&token));
		Ok(PreviewPreparation {
			reservation: Preparation { id, token },
			input,
		})
	}
	pub fn preview(&mut self, input: Input) -> io::Result<Preview> {
		let request = self.begin_preview(input)?;
		let prepared = Self::prepare_preview(request)?;
		self.finish_preview(prepared)
	}
	pub fn finish_preview(&mut self, prepared: PreparedPreview) -> io::Result<Preview> {
		let PreparedPreview {
			reservation,
			preview,
		} = prepared;
		if !self
			.preview_preparations
			.get(&reservation.id)
			.and_then(Weak::upgrade)
			.is_some_and(|token| Arc::ptr_eq(&token, &reservation.token))
		{
			return Err(io::Error::other("Project preview preparation expired"));
		}
		self.preview_preparations.remove(&reservation.id);
		if self.plans.len() >= 16 {
			self.plans.clear();
		}
		self.plans.insert(preview.id.clone(), preview.clone());
		Ok(preview)
	}
	pub fn generate(&mut self, id: &str, jobs: &mut Jobs) -> io::Result<Generation> {
		let request = self.begin_generation(id)?;
		let prepared = Self::prepare_generation(request)?;
		self.finish_generation(prepared, jobs)
	}
	pub fn begin_generation(&mut self, id: &str) -> io::Result<GenerationPreparation> {
		if self.storage_error.is_some() {
			return Err(io::Error::other(
				"Recover project history storage before generating",
			));
		}
		if self.has_preparation() || self.runs.iter().any(|r| r.state == "VALIDATING") {
			return Err(io::Error::other("A project is already being validated"));
		}
		let plan = self
			.plans
			.get(id)
			.cloned()
			.ok_or_else(|| io::Error::other("Preview expired; preview again"))?;
		let token = Arc::new(());
		self.preparation_admission = Arc::downgrade(&token);
		self.generation_preparation = Some((id.to_owned(), Arc::downgrade(&token)));
		Ok(GenerationPreparation {
			reservation: Preparation {
				id: id.to_owned(),
				token,
			},
			plan,
		})
	}
	pub fn finish_generation(
		&mut self,
		prepared: PreparedGeneration,
		jobs: &mut Jobs,
	) -> io::Result<Generation> {
		let PreparedGeneration {
			reservation,
			mut run,
			offline,
		} = prepared;
		if !self
			.generation_preparation
			.as_ref()
			.is_some_and(|(id, token)| {
				id == &reservation.id
					&& token
						.upgrade()
						.is_some_and(|token| Arc::ptr_eq(&token, &reservation.token))
			}) {
			return Err(io::Error::other("Project generation preparation expired"));
		}
		self.generation_preparation = None;
		let result = (|| {
			if self.storage_error.is_some() {
				return Err(io::Error::other(
					"Recover project history storage before generating",
				));
			}
			if run.destination.exists() {
				return Err(io::Error::other("Destination is occupied"));
			}
			let job = jobs.queue(
				&run.staging,
				Kind::Build,
				"game".into(),
				run.profile.clone(),
			)?;
			jobs.set_offline(&job.id, offline);
			run.job_id = job.id;
			self.runs.insert(0, run.clone());
			self.runs.truncate(50);
			self.plans.remove(&reservation.id);
			self.persist();
			Ok(())
		})();
		result.map_err(|error: io::Error| {
			io::Error::other(format!(
				"{error}; staging retained at {}",
				run.staging.display()
			))
		})?;
		Ok(run)
	}
	pub fn tick(&mut self, jobs: &mut Jobs) {
		let mut changed = false;
		for run in &mut self.runs {
			if run.state != "VALIDATING" {
				continue;
			}
			let Some(job) = jobs.runs.iter().find(|j| j.id == run.job_id) else {
				run.state = "INTERRUPTED".into();
				run.detail = Some("Validation job no longer retained".into());
				changed = true;
				continue;
			};
			if matches!(job.state, Status::Queued | Status::Running) {
				continue;
			}
			run.finished_ms = Some(now_ms());
			changed = true;
			if job.state != Status::Succeeded {
				run.state = match job.state {
					Status::Cancelled => "CANCELLED",
					Status::Interrupted => "INTERRUPTED",
					_ => "FAILED",
				}
				.into();
				run.detail = Some("Validation did not succeed; staging retained".into());
				continue;
			}
			let artifacts: Vec<_> = job
				.artifacts
				.iter()
				.filter_map(|a| {
					crate::relative_to(Path::new(a), &run.staging)
						.map(|p| p.to_string_lossy().into_owned())
				})
				.collect();
			let manifest = serde_json::json!({"template_version":1,"name":run.name,"engine_revision":run.revision,"source_snapshot":"copied local working tree","profile":run.profile,"target":run.target,"operation_id":run.job_id,"toolchain":job.toolchain,"artifacts":artifacts,"files":run.files,"platform":format!("{}-{}",std::env::consts::ARCH,std::env::consts::OS),"created_ms":run.created_ms,"validated_ms":run.finished_ms});
			let result = (|| -> io::Result<()> {
				if run.destination.exists() {
					return Err(io::Error::other(
						"Destination became occupied; it was not overwritten",
					));
				}
				save_json(&run.staging.join("generation-manifest.json"), &manifest)?;
				publish_new(&run.staging, &run.destination)?;
				Ok(())
			})();
			match result {
				Ok(()) => {
					run.state = "SUCCEEDED".into();
					run.detail = Some("Validated independent workspace is ready".into());
					engine_logger::info!(target: "admin::project_builder", "Generated project published: job={}", run.job_id);
				}
				Err(e) => {
					run.state = "FAILED".into();
					run.detail = Some(e.to_string());
					engine_logger::error!(target: "admin::project_builder", "Generated project publication failed: job={}, kind={:?}", run.job_id, e.kind());
				}
			}
		}
		if changed {
			self.persist();
		}
		for run in self.runs.iter().filter(|r| r.state == "SUCCEEDED") {
			jobs.relocate_artifacts(&run.job_id, &run.staging, &run.destination);
		}
	}
}
#[cfg(test)]
#[path = "../tests/unit/builder/mod.rs"]
mod tests;
