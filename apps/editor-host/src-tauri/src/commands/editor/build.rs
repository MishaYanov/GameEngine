//! Trusted native commands for staging, finishing, and inspecting a standalone export.

use super::*;
use crate::{
	release_resources::EditorResources,
	state::project_build::{
		BuildDiagnosticCode, BuildReservation, ProjectBuildOutput, ProjectBuildReadback,
	},
};
use std::path::PathBuf;

const BUILD_CHANGED: &str = "editor://project-build-changed";

pub type ProjectBuildDto = ProjectBuildOutput;

#[derive(Serialize)]
pub struct PreparedBuildDto {
	build_id: u64,
	#[serde(flatten)]
	output: ProjectBuildOutput,
}

#[tauri::command]
pub async fn editor_prepare_project_build(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	expected_revision: u64,
	destination: String,
) -> Result<PreparedBuildDto, String> {
	trusted_window(&window)?;
	let resources = EditorResources::for_window(&window)?;
	let (reservation, staged, _) = stage_build(
		&window,
		state,
		resources,
		expected_revision,
		destination,
		false,
	)
	.await?;
	Ok(PreparedBuildDto {
		build_id: reservation.id(),
		output: ProjectBuildOutput::from(&staged),
	})
}

#[tauri::command]
pub async fn editor_finish_project_build(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	build_id: u64,
) -> Result<ProjectBuildDto, String> {
	trusted_window(&window)?;
	let resources = EditorResources::for_window(&window)?;
	let builds = Arc::clone(&state.project_build);
	let (reservation, staged) = workspace(state, false, None, move |workspace| {
		BuildReservation::finish(builds, build_id, workspace.generation)
	})
	.await?;
	finish_build(&window, reservation, staged, move || resources.player()).await
}

#[tauri::command]
pub async fn editor_project_build_status(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	workspace_generation: u64,
) -> Result<ProjectBuildReadback, String> {
	trusted_window(&window)?;
	let builds = state
		.project_build
		.lock()
		.map_err(|_| "Build state lock failed")?;
	Ok(builds.readback(workspace_generation))
}

#[tauri::command]
pub async fn editor_discard_project_build(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	build_id: u64,
) -> Result<(), String> {
	trusted_window(&window)?;
	let builds = Arc::clone(&state.project_build);
	workspace(state, false, None, move |workspace| {
		builds
			.lock()
			.map_err(|_| "Build state lock failed")?
			.discard(workspace.generation, build_id)
	})
	.await?;
	notify(&window);
	Ok(())
}

#[tauri::command]
pub async fn editor_build_project(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	expected_revision: u64,
	destination: String,
) -> Result<ProjectBuildDto, String> {
	trusted_window(&window)?;
	let resources = EditorResources::for_window(&window)?;
	let (reservation, staged, player) = stage_build(
		&window,
		state,
		resources,
		expected_revision,
		destination,
		true,
	)
	.await?;
	finish_build(&window, reservation, staged, move || Ok(player)).await
}

#[derive(Serialize)]
pub struct ProjectRunDto {
	path: String,
	process_id: u32,
}

#[tauri::command]
pub async fn editor_run_build(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	expected_path: String,
) -> Result<ProjectRunDto, String> {
	trusted_window(&window)?;
	let builds = Arc::clone(&state.project_build);
	let installed = workspace(state, false, None, move |workspace| {
		let builds = builds.lock().map_err(|_| "Build state lock failed")?;
		Ok(builds.installed(workspace.generation))
	})
	.await?;
	tauri::async_runtime::spawn_blocking(move || {
		let mut command = build_launch_command(installed.as_ref(), Path::new(&expected_path))?;
		let child = command.spawn().map_err(|error| {
			engine_logger::error!(target: "project_build", "Built game launch failed: operation=spawn, kind={:?}", error.kind());
			format!("Could not run built game: {error}")
		})?;
		Ok(ProjectRunDto { path: expected_path, process_id: child.id() })
	})
	.await
	.map_err(|error| format!("Game launch worker failed: {error}"))?
}

async fn stage_build(
	window: &WebviewWindow,
	state: State<'_, EditorState>,
	resources: EditorResources,
	expected_revision: u64,
	destination: String,
	finish_immediately: bool,
) -> Result<(BuildReservation, compiler::StagedProject, Option<PathBuf>), String> {
	let builds = Arc::clone(&state.project_build);
	let reporter = window.clone();
	let validated = workspace(state, false, Some("prepare_build"), move |workspace| {
		let mut reservation =
			BuildReservation::start(builds, workspace.generation, expected_revision)?;
		notify(&reporter);
		let prepared = workspace
			.prepare_build(expected_revision)
			.map_err(|message| reservation.fail(BuildDiagnosticCode::ValidationFailed, message))?;
		Ok((reservation, prepared))
	})
	.await;
	let (mut reservation, prepared) = match validated {
		Ok(validated) => validated,
		Err(error) => {
			notify(window);
			return Err(error);
		}
	};
	reservation.begin_staging()?;
	notify(window);
	let result = tauri::async_runtime::spawn_blocking(move || {
		let player = resources
			.player()
			.map_err(|message| reservation.fail(BuildDiagnosticCode::PlayerUnavailable, message))?;
		let staged = compiler::stage_prepared_project(&prepared, &destination).map_err(|message| {
			engine_logger::error!(target: "project_build", "Build failed: operation=stage_prepared_project");
			reservation.fail(BuildDiagnosticCode::StagingFailed, message)
		})?;
		reservation.record_staged(&staged, finish_immediately)?;
		Ok((reservation, staged, player))
	})
	.await;
	notify(window);
	result.map_err(|error| format!("Project staging worker failed: {error}"))?
}

async fn finish_build(
	window: &WebviewWindow,
	mut reservation: BuildReservation,
	staged: compiler::StagedProject,
	player: impl FnOnce() -> Result<Option<PathBuf>, String> + Send + 'static,
) -> Result<ProjectBuildDto, String> {
	notify(window);
	let result = tauri::async_runtime::spawn_blocking(move || {
		let player = player()
			.map_err(|message| reservation.fail(BuildDiagnosticCode::PlayerUnavailable, message))?;
		complete_export(reservation, staged, player.as_deref())
	})
	.await;
	notify(window);
	result.map_err(|error| format!("Project build worker failed: {error}"))?
}

fn complete_export(
	mut reservation: BuildReservation,
	staged: compiler::StagedProject,
	player: Option<&Path>,
) -> Result<ProjectBuildDto, String> {
	let output = ProjectBuildOutput::from(&staged);
	let ProjectBuildOutput {
		asset_count,
		asset_bytes,
		..
	} = output;
	engine_logger::info!(target: "project_build", "Build started: assets={asset_count}, bytes={asset_bytes}");
	let installed = match player {
		Some(executable) => compiler::complete_staged_project_with_player(staged, executable),
		None => compiler::complete_staged_project(staged),
	}
	.map_err(|message| {
		engine_logger::error!(target: "project_build", "Build failed: operation=finish_staged_project");
		reservation.fail(BuildDiagnosticCode::FinishFailed, message)
	})?;
	reservation.record_success(installed)?;
	engine_logger::info!(target: "project_build", "Build completed: assets={asset_count}, bytes={asset_bytes}");
	Ok(output)
}

fn notify(window: &WebviewWindow) {
	let _ = window.emit(BUILD_CHANGED, ());
}

fn build_launch_command(
	build: Option<&compiler::InstalledProject>,
	expected_path: &Path,
) -> Result<Command, String> {
	let build = build.ok_or("Build a project before running its exported game")?;
	if build.path() != expected_path {
		return Err("The latest completed build changed; run the current build instead".into());
	}
	let mut command = Command::new(build.verified_executable()?);
	command.current_dir(build.path()).arg("--player");
	Ok(command)
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/tests_build_launch.rs"]
mod tests_build_launch;
