use super::*;

#[test]
fn launch_requires_a_successful_build_and_an_existing_player() {
	let path = Path::new(env!("CARGO_MANIFEST_DIR"));
	assert!(build_launch_command(None, path)
		.unwrap_err()
		.contains("Build a project"));
	assert!(build_launch_command(Some(path), path).is_err());
	assert!(build_launch_command(Some(path), Path::new("outdated"))
		.unwrap_err()
		.contains("latest completed build changed"));
}

#[test]
fn launch_uses_the_exported_executable_and_its_asset_working_directory() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("../../../target")
		.join(format!("qa-build-launch-{}", std::process::id()));
	std::fs::create_dir(&root).unwrap();
	let executable = root.join(format!("game{}", std::env::consts::EXE_SUFFIX));
	std::fs::write(&executable, b"fixture; do not execute").unwrap();
	let result = build_launch_command(Some(&root), &root);
	let expected = root.canonicalize().unwrap();
	std::fs::remove_dir_all(&root).unwrap();
	let command = result.unwrap();
	let expected_executable = expected.join(executable.file_name().unwrap());
	assert_eq!(command.get_program(), expected_executable.as_os_str());
	assert_eq!(command.get_current_dir(), Some(expected.as_path()));
	assert_eq!(command.get_args().collect::<Vec<_>>(), ["--player"]);
}

#[test]
fn failed_finish_can_retry_its_frozen_token_but_never_revives_a_superseded_build() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("../../../target")
		.join(format!("qa-build-token-{}", std::process::id()));
	std::fs::create_dir(&root).unwrap();
	let source = compiler::create_authoring_project("Token Demo", root.to_str().unwrap()).unwrap();
	let project = runtime::project::ProjectData::load(source.join("project.json")).unwrap();
	let prepared = compiler::prepare_project(&project, &source.join("assets")).unwrap();
	let exports = root.join("exports");
	std::fs::create_dir(&exports).unwrap();
	let staged = compiler::stage_prepared_project(&prepared, exports.to_str().unwrap()).unwrap();
	let pending = Mutex::new(ProjectBuildState {
		latest_id: 7,
		pending: Some(PendingProjectBuild {
			id: 7,
			staged: staged.clone(),
		}),
	});
	assert!(
		finish_prepared_build(&pending, 6, |_| panic!("stale token must not compile")).is_err()
	);
	assert!(finish_prepared_build(&pending, 7, |_| Err("Compiler unavailable".into())).is_err());
	assert_eq!(pending.lock().unwrap().pending.as_ref().unwrap().id, 7);
	let finished = finish_prepared_build(&pending, 7, |staged| {
		Ok(ProjectBuildDto {
			path: staged.path().to_string_lossy().into_owned(),
			asset_count: staged.asset_count(),
			asset_bytes: staged.asset_bytes(),
		})
	})
	.unwrap();
	assert_eq!(finished.path, staged.path().to_string_lossy());
	assert!(pending.lock().unwrap().pending.is_none());
	assert!(
		finish_prepared_build(&pending, 7, |_| panic!("consumed token must not compile")).is_err()
	);
	{
		let mut state = pending.lock().unwrap();
		state.latest_id = 8;
		state.pending = Some(PendingProjectBuild { id: 8, staged });
	}
	assert!(finish_prepared_build(&pending, 8, |_| {
		// A newer preparation was already claimed while this compilation ran.
		pending.lock().unwrap().latest_id = 9;
		Err("Older compilation failed".into())
	})
	.is_err());
	assert!(pending.lock().unwrap().pending.is_none());
	std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn prepared_build_json_carries_the_token_and_export_snapshot_statistics() {
	let prepared = PreparedBuildDto {
		build_id: 3,
		path: "D:/Exports/Demo".into(),
		asset_count: 5,
		asset_bytes: 128,
	};
	assert_eq!(
		serde_json::to_value(prepared).unwrap(),
		serde_json::json!({
			"build_id":3, "path":"D:/Exports/Demo", "asset_count":5, "asset_bytes":128
		})
	);
}
