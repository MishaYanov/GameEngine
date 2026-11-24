//! One export reservation and one latest playable output for the active editor process.

use serde::Serialize;
use std::sync::{
	atomic::{AtomicU64, Ordering},
	Arc, Mutex,
};

const MAX_DIAGNOSTIC_BYTES: usize = 4_096;
const MAX_WIRE_BUILD_ID: u64 = (1 << 53) - 1;
static NEXT_BUILD_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Serialize)]
pub struct ProjectBuildOutput {
	pub(crate) path: String,
	pub(crate) asset_count: usize,
	pub(crate) asset_bytes: u64,
}

impl From<&compiler::StagedProject> for ProjectBuildOutput {
	fn from(staged: &compiler::StagedProject) -> Self {
		Self {
			path: staged.path().to_string_lossy().into_owned(),
			asset_count: staged.asset_count(),
			asset_bytes: staged.asset_bytes(),
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BuildPhase {
	Validating,
	Staging,
	Staged,
	Finishing,
	Succeeded,
	Failed,
}

impl BuildPhase {
	fn running(self) -> bool {
		matches!(self, Self::Validating | Self::Staging | Self::Finishing)
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BuildDiagnosticCode {
	ValidationFailed,
	PlayerUnavailable,
	StagingFailed,
	FinishFailed,
	WorkerInterrupted,
}

#[derive(Clone, Serialize)]
pub(crate) struct BuildDiagnostic {
	pub(crate) code: BuildDiagnosticCode,
	pub(crate) message: String,
}

impl BuildDiagnostic {
	fn new(code: BuildDiagnosticCode, mut message: String) -> Self {
		if message.len() > MAX_DIAGNOSTIC_BYTES {
			let mut end = MAX_DIAGNOSTIC_BYTES - '…'.len_utf8();
			while !message.is_char_boundary(end) {
				end -= 1;
			}
			message.truncate(end);
			message.push('…');
		}
		Self { code, message }
	}
}

#[derive(Clone, Serialize)]
pub(crate) struct ProjectBuildStatus {
	pub(crate) build_id: u64,
	workspace_generation: u64,
	source_revision: u64,
	pub(crate) phase: BuildPhase,
	pub(crate) output: Option<ProjectBuildOutput>,
	pub(crate) diagnostic: Option<BuildDiagnostic>,
	pub(crate) retryable: bool,
}

#[derive(Serialize)]
pub struct ProjectBuildReadback {
	pub(crate) occupied: bool,
	pub(crate) current: Option<ProjectBuildStatus>,
}

struct ProjectBuildJob {
	status: ProjectBuildStatus,
	staged: Option<compiler::StagedProject>,
}

#[derive(Default)]
pub(crate) struct ProjectBuildState {
	job: Option<ProjectBuildJob>,
	installed: Option<(u64, compiler::InstalledProject)>,
}

impl ProjectBuildState {
	fn occupied(&self, workspace_generation: u64) -> bool {
		self.job.as_ref().is_some_and(|job| {
			job.status.phase.running()
				|| (job.status.phase == BuildPhase::Staged
					&& job.status.workspace_generation == workspace_generation)
		})
	}

	pub(crate) fn readback(&self, workspace_generation: u64) -> ProjectBuildReadback {
		ProjectBuildReadback {
			occupied: self.occupied(workspace_generation),
			current: self
				.job
				.as_ref()
				.filter(|job| job.status.workspace_generation == workspace_generation)
				.map(|job| job.status.clone()),
		}
	}

	pub(crate) fn discard(
		&mut self,
		workspace_generation: u64,
		build_id: u64,
	) -> Result<(), String> {
		let discardable = self.job.as_ref().is_some_and(|job| {
			job.status.workspace_generation == workspace_generation
				&& job.status.build_id == build_id
				&& matches!(job.status.phase, BuildPhase::Staged | BuildPhase::Failed)
				&& job.staged.is_some()
		});
		if !discardable {
			return Err("Only a staged export in this project can be discarded".into());
		}
		self.job = None;
		Ok(())
	}

	pub(crate) fn installed(&self, generation: u64) -> Option<compiler::InstalledProject> {
		self.installed
			.as_ref()
			.filter(|(owner, _)| *owner == generation)
			.map(|(_, installed)| installed.clone())
	}
}

/// Moves an export through its worker phases. Dropping a worker always releases its reservation.
pub(crate) struct BuildReservation {
	state: Arc<Mutex<ProjectBuildState>>,
	id: u64,
	armed: bool,
}

impl BuildReservation {
	pub(crate) fn start(
		state: Arc<Mutex<ProjectBuildState>>,
		workspace_generation: u64,
		source_revision: u64,
	) -> Result<Self, String> {
		let mut builds = state.lock().map_err(|_| "Build state lock failed")?;
		if builds.occupied(workspace_generation) {
			return Err("Finish or discard the current export before starting another".into());
		}
		let id = NEXT_BUILD_ID
			.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| {
				(id <= MAX_WIRE_BUILD_ID).then(|| id + 1)
			})
			.map_err(|_| "Build token space is exhausted")?;
		builds.job = Some(ProjectBuildJob {
			status: ProjectBuildStatus {
				build_id: id,
				workspace_generation,
				source_revision,
				phase: BuildPhase::Validating,
				output: None,
				diagnostic: None,
				retryable: false,
			},
			staged: None,
		});
		drop(builds);
		Ok(Self {
			state,
			id,
			armed: true,
		})
	}

	pub(crate) fn finish(
		state: Arc<Mutex<ProjectBuildState>>,
		build_id: u64,
		workspace_generation: u64,
	) -> Result<(Self, compiler::StagedProject), String> {
		let mut builds = state.lock().map_err(|_| "Build state lock failed")?;
		let job = builds
			.job
			.as_mut()
			.filter(|job| {
				job.status.build_id == build_id
					&& job.status.workspace_generation == workspace_generation
					&& matches!(job.status.phase, BuildPhase::Staged | BuildPhase::Failed)
			})
			.ok_or("Prepared export is missing, stale, or already finishing")?;
		let staged = job
			.staged
			.clone()
			.ok_or("Export files have not been staged")?;
		job.status.phase = BuildPhase::Finishing;
		job.status.diagnostic = None;
		job.status.retryable = false;
		drop(builds);
		let reservation = Self {
			state,
			id: build_id,
			armed: true,
		};
		Ok((reservation, staged))
	}

	pub(crate) fn id(&self) -> u64 {
		self.id
	}

	pub(crate) fn begin_staging(&self) -> Result<(), String> {
		self.update(|job| job.status.phase = BuildPhase::Staging)
	}

	pub(crate) fn record_staged(
		&mut self,
		staged: &compiler::StagedProject,
		finish_immediately: bool,
	) -> Result<(), String> {
		self.update(|job| {
			job.status.output = Some(ProjectBuildOutput::from(staged));
			job.status.phase = if finish_immediately {
				BuildPhase::Finishing
			} else {
				BuildPhase::Staged
			};
			job.status.retryable = false;
			job.staged = Some(staged.clone());
		})?;
		self.armed = finish_immediately;
		Ok(())
	}

	pub(crate) fn record_success(
		mut self,
		installed: compiler::InstalledProject,
	) -> Result<(), String> {
		let mut builds = self.state.lock().map_err(|_| "Build state lock failed")?;
		let job = builds
			.job
			.as_mut()
			.filter(|job| job.status.build_id == self.id)
			.ok_or("Export reservation no longer exists")?;
		job.status.phase = BuildPhase::Succeeded;
		job.staged = None;
		builds.installed = Some((job.status.workspace_generation, installed));
		self.armed = false;
		Ok(())
	}

	pub(crate) fn fail(&mut self, code: BuildDiagnosticCode, message: String) -> String {
		let diagnostic = BuildDiagnostic::new(code, message);
		// Leave the guard armed if the lock cannot be acquired.
		if self
			.update(|job| Self::set_failure(job, diagnostic.clone()))
			.is_ok()
		{
			self.armed = false;
		}
		diagnostic.message
	}

	fn update(&self, action: impl FnOnce(&mut ProjectBuildJob)) -> Result<(), String> {
		let mut builds = self.state.lock().map_err(|_| "Build state lock failed")?;
		let job = builds
			.job
			.as_mut()
			.filter(|job| job.status.build_id == self.id)
			.ok_or("Export reservation no longer exists")?;
		action(job);
		Ok(())
	}

	fn set_failure(job: &mut ProjectBuildJob, diagnostic: BuildDiagnostic) {
		job.status.phase = BuildPhase::Failed;
		job.status.diagnostic = Some(diagnostic);
		job.status.retryable = job.staged.is_some();
	}
}

impl Drop for BuildReservation {
	fn drop(&mut self) {
		if self.armed {
			let _ = self.update(|job| {
				Self::set_failure(
					job,
					BuildDiagnostic::new(
						BuildDiagnosticCode::WorkerInterrupted,
						"The export worker stopped before completion.".into(),
					),
				);
			});
		}
	}
}
