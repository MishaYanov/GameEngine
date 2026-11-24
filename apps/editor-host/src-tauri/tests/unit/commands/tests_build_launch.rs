use super::*;
use crate::state::project_build::{BuildPhase, ProjectBuildState};

#[test]
fn launch_requires_a_successful_build() {
	let path = Path::new(env!("CARGO_MANIFEST_DIR"));
	assert!(build_launch_command(None, path)
		.unwrap_err()
		.contains("Build a project"));
}

#[test]
fn finishing_an_installed_editor_export_copies_the_supplied_player() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("../../../target")
		.join(format!("qa-bundled-player-finish-{}", std::process::id()));
	std::fs::create_dir_all(root.parent().unwrap()).unwrap();
	std::fs::create_dir(&root).unwrap();
	let destination = root.to_str().unwrap();
	let source = compiler::create_blank_project("Bundled Player Demo", destination).unwrap();
	let project = runtime::project::ProjectData::load(source.join("project.json")).unwrap();
	let prepared = compiler::prepare_project(&project, &source.join("assets")).unwrap();
	let exports = root.join("exports");
	std::fs::create_dir(&exports).unwrap();
	let staged = compiler::stage_prepared_project(&prepared, exports.to_str().unwrap()).unwrap();
	let player = root.join("player-resource.exe");
	let bytes = b"prebuilt player fixture";
	std::fs::write(&player, bytes).unwrap();

	let shared = Arc::new(Mutex::new(ProjectBuildState::default()));
	let mut reservation = BuildReservation::start(Arc::clone(&shared), 3, 9).unwrap();
	reservation.begin_staging().unwrap();
	reservation.record_staged(&staged, true).unwrap();
	let result = complete_export(reservation, staged, Some(&player)).unwrap();
	let state = shared.lock().unwrap();
	assert_eq!(
		state.readback(3).current.unwrap().phase,
		BuildPhase::Succeeded
	);
	assert!(state.installed(4).is_none());
	let installed = state.installed(3);
	let command = build_launch_command(installed.as_ref(), Path::new(&result.path)).unwrap();
	assert_eq!(std::fs::read(command.get_program()).unwrap(), bytes);
	assert_eq!(std::fs::read(&player).unwrap(), bytes);
	drop(installed);
	std::fs::remove_dir_all(&root).unwrap();
}

#[test]
#[ignore = "builds a real release player"]
fn launch_uses_the_exported_executable_and_its_asset_working_directory() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("../../../target")
		.join(format!("qa-build-launch-{}", std::process::id()));
	std::fs::create_dir(&root).unwrap();
	let source = compiler::create_authoring_project("Launch Demo", root.to_str().unwrap()).unwrap();
	let project = runtime::project::ProjectData::load(source.join("project.json")).unwrap();
	let prepared = compiler::prepare_project(&project, &source.join("assets")).unwrap();
	let exports = root.join("exports");
	std::fs::create_dir(&exports).unwrap();
	let staged = compiler::stage_prepared_project(&prepared, exports.to_str().unwrap()).unwrap();
	let installed = compiler::complete_staged_project(staged).unwrap();
	let path = installed.path();
	assert!(
		build_launch_command(Some(&installed), Path::new("outdated"))
			.unwrap_err()
			.contains("latest completed build changed")
	);

	let command = build_launch_command(Some(&installed), path).unwrap();
	let executable = path.join(format!("game{}", std::env::consts::EXE_SUFFIX));
	assert_eq!(command.get_program(), executable.as_os_str());
	assert_eq!(command.get_current_dir(), Some(path));
	assert_eq!(command.get_args().collect::<Vec<_>>(), ["--player"]);
	let mut player = std::fs::read(&executable).unwrap();
	player[0] ^= 1;
	std::fs::write(&executable, player).unwrap();
	assert!(build_launch_command(Some(&installed), path).is_err());
	std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn staged_build_reservation_serializes_finishing_and_keeps_a_retry_for_the_same_files() {
	let root = std::env::temp_dir()
		.canonicalize()
		.unwrap()
		.join(format!("editor-build-token-{}", std::process::id()));
	std::fs::create_dir(&root).unwrap();
	let source = compiler::create_blank_project("Token Demo", root.to_str().unwrap()).unwrap();
	let project = runtime::project::ProjectData::load(source.join("project.json")).unwrap();
	let prepared = compiler::prepare_project(&project, &source.join("assets")).unwrap();
	let exports = root.join("exports");
	std::fs::create_dir(&exports).unwrap();
	let staged = compiler::stage_prepared_project(&prepared, exports.to_str().unwrap()).unwrap();
	let shared = Arc::new(Mutex::new(ProjectBuildState::default()));
	let mut reservation = BuildReservation::start(Arc::clone(&shared), 4, 27).unwrap();
	let build_id = reservation.id();
	let status = shared.lock().unwrap().readback(4);
	assert!(status.occupied);
	assert_eq!(status.current.unwrap().phase, BuildPhase::Validating);
	reservation.begin_staging().unwrap();
	assert!(BuildReservation::start(Arc::clone(&shared), 4, 40).is_err());
	assert!(BuildReservation::start(Arc::clone(&shared), 5, 40).is_err());

	reservation.record_staged(&staged, false).unwrap();
	drop(reservation);
	assert!(BuildReservation::start(Arc::clone(&shared), 4, 40).is_err());
	assert!(BuildReservation::finish(Arc::clone(&shared), build_id, 5).is_err());
	assert!(BuildReservation::finish(Arc::clone(&shared), build_id + 1, 4).is_err());
	let (mut finishing, frozen) =
		BuildReservation::finish(Arc::clone(&shared), build_id, 4).unwrap();
	assert_eq!(frozen.path(), staged.path());

	// Status queries and rejection of a competing export do not need the blocking worker.
	let other = Arc::clone(&shared);
	std::thread::spawn(move || {
		let status = other.lock().unwrap().readback(5);
		assert!(status.occupied);
		assert!(status.current.is_none());
		assert!(BuildReservation::start(other, 5, 40).is_err());
	})
	.join()
	.unwrap();
	assert!(BuildReservation::finish(Arc::clone(&shared), build_id, 4).is_err());
	assert!(shared.lock().unwrap().discard(4, build_id).is_err());

	let text = finishing.fail(BuildDiagnosticCode::FinishFailed, "🐸".repeat(3_000));
	assert!(text.ends_with('…'));
	assert!(text.len() <= 4_096);
	drop(finishing);
	let status = shared.lock().unwrap().readback(4);
	assert!(!status.occupied);
	let job = status.current.unwrap();
	assert!(job.retryable);
	assert_eq!(job.phase, BuildPhase::Failed);
	assert_eq!(
		job.diagnostic.unwrap().code,
		BuildDiagnosticCode::FinishFailed
	);
	let (retry, frozen) = BuildReservation::finish(Arc::clone(&shared), build_id, 4).unwrap();
	assert_eq!(frozen.path(), staged.path());
	drop(retry);
	let status = shared.lock().unwrap().readback(4).current.unwrap();
	assert!(status.retryable);
	assert_eq!(
		status.diagnostic.unwrap().code,
		BuildDiagnosticCode::WorkerInterrupted
	);
	assert!(shared.lock().unwrap().discard(5, build_id).is_err());
	shared.lock().unwrap().discard(4, build_id).unwrap();
	assert!(!shared.lock().unwrap().readback(4).occupied);
	assert!(staged.path().join("project.json").is_file());

	let new = BuildReservation::start(Arc::clone(&shared), 5, 40).unwrap();
	assert!(BuildReservation::finish(Arc::clone(&shared), build_id, 4).is_err());
	assert_eq!(
		shared.lock().unwrap().readback(5).current.unwrap().build_id,
		new.id()
	);
	drop(new);
	std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn prepared_build_json_carries_the_token_and_export_snapshot_statistics() {
	let prepared = PreparedBuildDto {
		build_id: 3,
		output: ProjectBuildOutput {
			path: "D:/Exports/Demo".into(),
			asset_count: 5,
			asset_bytes: 128,
		},
	};
	assert_eq!(
		serde_json::to_value(prepared).unwrap(),
		serde_json::json!({
			"build_id":3, "path":"D:/Exports/Demo", "asset_count":5, "asset_bytes":128
		})
	);
}
