use super::*;

#[derive(Serialize)]
pub struct ProjectBuildDto {
	path: String,
	asset_count: usize,
	asset_bytes: u64,
}

#[derive(Serialize)]
pub struct PreparedBuildDto {
	build_id: u64,
	path: String,
	asset_count: usize,
	asset_bytes: u64,
}

static NEXT_BUILD_ID: AtomicU64 = AtomicU64::new(1);

#[tauri::command]
pub async fn editor_prepare_project_build(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	expected_revision: u64,
	destination: String,
) -> Result<PreparedBuildDto, String> {
	trusted_window(&window)?;
	let pending_build = Arc::clone(&state.pending_build);
	let prepared = workspace(state, false, Some("prepare_build"), move |w| {
		w.prepare_build(expected_revision)
	})
	.await?;
	tauri::async_runtime::spawn_blocking(move || {
		let mut pending = pending_build
			.lock()
			.map_err(|_| "Build state lock failed")?;
		let staged = stage_project_build(&prepared, &destination)?;
		let build_id = NEXT_BUILD_ID
			.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
			.map_err(|_| "Build token space is exhausted")?;
		let result = PreparedBuildDto {
			build_id,
			path: staged.path().to_string_lossy().into_owned(),
			asset_count: staged.asset_count(),
			asset_bytes: staged.asset_bytes(),
		};
		// Only one unclaimed token is retained. New preparation supersedes an
		// abandoned token; its already-created output remains available to inspect.
		pending.latest_id = build_id;
		pending.pending = Some(PendingProjectBuild {
			id: build_id,
			staged,
		});
		Ok(result)
	})
	.await
	.map_err(|error| format!("Project staging worker failed: {error}"))?
}

#[tauri::command]
pub async fn editor_finish_project_build(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	build_id: u64,
) -> Result<ProjectBuildDto, String> {
	trusted_window(&window)?;
	let pending_build = Arc::clone(&state.pending_build);
	let last_build = Arc::clone(&state.last_build);
	tauri::async_runtime::spawn_blocking(move || {
		finish_prepared_build(&pending_build, build_id, |staged| {
			finish_project_build(staged, &last_build)
		})
	})
	.await
	.map_err(|error| format!("Project build worker failed: {error}"))?
}

fn finish_prepared_build(
	pending_build: &Mutex<ProjectBuildState>,
	build_id: u64,
	finish: impl FnOnce(compiler::StagedProject) -> Result<ProjectBuildDto, String>,
) -> Result<ProjectBuildDto, String> {
	let staged = {
		let mut state = pending_build
			.lock()
			.map_err(|_| "Build state lock failed")?;
		if state.pending.as_ref().map(|build| build.id) != Some(build_id) {
			return Err("Prepared build is missing, superseded, or already compiling".into());
		}
		state.pending.take().expect("matched pending build").staged
	};
	let retry = staged.clone();
	let result = finish(staged);
	if result.is_err() {
		let mut state = pending_build
			.lock()
			.map_err(|_| "Build state lock failed")?;
		if state.latest_id == build_id && state.pending.is_none() {
			state.pending = Some(PendingProjectBuild {
				id: build_id,
				staged: retry,
			});
		}
	}
	result
}

fn stage_project_build(
	prepared: &compiler::PreparedProject,
	destination: &str,
) -> Result<compiler::StagedProject, String> {
	compiler::stage_prepared_project(prepared, destination).inspect_err(|_| {
		engine_logger::error!(target: "project_build", "Build failed: operation=stage_prepared_project");
	})
}

fn finish_project_build(
	staged: compiler::StagedProject,
	last_build: &Mutex<Option<compiler::InstalledProject>>,
) -> Result<ProjectBuildDto, String> {
	let asset_count = staged.asset_count();
	let asset_bytes = staged.asset_bytes();
	engine_logger::info!(target:"project_build","Build started: assets={asset_count}, bytes={asset_bytes}");
	let installed = compiler::complete_staged_project(staged).inspect_err(|_| {
		engine_logger::error!(target:"project_build","Build failed: operation=finish_staged_project");
	})?;
	let path = installed.path().to_string_lossy().into_owned();
	*last_build.lock().map_err(|_| "Build state lock failed")? = Some(installed);
	engine_logger::info!(target:"project_build","Build completed: assets={asset_count}, bytes={asset_bytes}");
	Ok(ProjectBuildDto {
		path,
		asset_count,
		asset_bytes,
	})
}

#[tauri::command]
pub async fn editor_build_project(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	expected_revision: u64,
	destination: String,
) -> Result<ProjectBuildDto, String> {
	trusted_window(&window)?;
	let last_build = Arc::clone(&state.last_build);
	let prepared = workspace(state, false, Some("prepare_build"), move |w| {
		w.prepare_build(expected_revision)
	})
	.await?;
	tauri::async_runtime::spawn_blocking(move || {
		let staged = stage_project_build(&prepared, &destination)?;
		finish_project_build(staged, &last_build)
	})
	.await
	.map_err(|e| format!("Project build worker failed: {e}"))?
}

#[derive(Serialize)]
pub struct ProjectRunDto {
	path: String,
	process_id: u32,
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

#[tauri::command]
pub async fn editor_run_build(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	expected_path: String,
) -> Result<ProjectRunDto, String> {
	trusted_window(&window)?;
	let last_build = Arc::clone(&state.last_build);
	tauri::async_runtime::spawn_blocking(move || {
		let latest = last_build.lock().map_err(|_| "Build state lock failed")?;
		let mut command = build_launch_command(latest.as_ref(), Path::new(&expected_path))?;
		let child = command
			.spawn()
			.map_err(|error| {
				engine_logger::error!(target: "project_build", "Built game launch failed: operation=spawn, kind={:?}", error.kind());
				format!("Could not run built game: {error}")
			})?;
		Ok(ProjectRunDto {
			path: expected_path,
			process_id: child.id(),
		})
	})
		.await
		.map_err(|error| format!("Game launch worker failed: {error}"))?
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/tests_build_launch.rs"]
mod tests_build_launch;
