use super::*;
use crate::processes::Processes;
fn fixture(label: &str) -> (PathBuf, PathBuf) {
	let root = crate::tests::test_directory(&format!("builder-{label}"));
	let source = root.join("source");
	fs::create_dir_all(&source).unwrap();
	for (folder, name) in [
		("apps/game", "game"),
		("crates/logger", "engine-logger"),
		("crates/renderer", "renderer"),
		("crates/runtime", "runtime"),
		("crates/bridge", "bridge"),
		("crates/engine-core", "engine-core"),
	] {
		fs::create_dir_all(source.join(folder).join("src")).unwrap();
		fs::write(
			source.join(folder).join("Cargo.toml"),
			format!("[package]\nname='{name}'\nversion='0.1.0'\nedition='2021'\n"),
		)
		.unwrap();
		fs::write(
			source
				.join(folder)
				.join("src")
				.join(if name == "game" { "main.rs" } else { "lib.rs" }),
			if name == "game" {
				"fn main() {}"
			} else {
				"pub fn ready() {}"
			},
		)
		.unwrap();
	}
	fs::create_dir_all(source.join("apps/game/assets")).unwrap();
	fs::write(source.join("apps/game/assets/sample.png"), b"fixture").unwrap();
	fs::write(source.join("apps/game/assets/.env"), b"excluded").unwrap();
	fs::create_dir_all(source.join("crates/renderer/shaders")).unwrap();
	fs::write(source.join("crates/renderer/build.rs"), "fn main() {}").unwrap();
	(root, source)
}
fn input(source: &Path, destination: &Path) -> Input {
	Input {
		name: "fixture-game".into(),
		source: source.to_string_lossy().into_owned(),
		destination: destination.to_string_lossy().into_owned(),
		profile: "dev".into(),
		target: "local".into(),
		offline: true,
	}
}
#[test]
fn preview_rejects_occupied_nested_and_changed_source() {
	let (root, source) = fixture("paths");
	let mut builder = Builder::new(&root.join("admin"));
	assert!(
		builder
			.preview(input(&source, &source.join("output")))
			.is_err()
	);
	assert!(builder.preview(input(&source, &source)).is_err());
	let plan = builder
		.preview(input(&source, &root.join("output")))
		.unwrap();
	assert!(!plan.files.iter().any(|f| f.path.ends_with(".env")));
	assert!(
		plan.files
			.iter()
			.any(|f| f.path == "crates/logger/Cargo.toml")
	);
	fs::write(source.join("apps/game/src/main.rs"), "fn main(){panic!();}").unwrap();
	let mut jobs = Jobs::new(&root.join("admin"));
	assert!(builder.generate(&plan.id, &mut jobs).is_err());
	assert!(!root.join("output").exists());
	assert!(jobs.runs.is_empty());
}

#[test]
fn preparation_reservations_release_after_drop_error_and_worker_panic() {
	let (root, source) = fixture("reservations");
	let mut builder = Builder::new(&root);
	let request = builder
		.begin_preview(input(&source, &root.join("output")))
		.unwrap();
	assert!(builder.has_preparation());
	drop(request);
	assert!(!builder.has_preparation());
	let request = builder.begin_preview(input(&source, &source)).unwrap();
	assert!(Builder::prepare_preview(request).is_err());
	assert!(!builder.has_preparation());
	let plan = builder
		.preview(input(&source, &root.join("output")))
		.unwrap();
	let request = builder.begin_generation(&plan.id).unwrap();
	assert!(builder.begin_generation(&plan.id).is_err());
	assert!(
		std::thread::spawn(move || {
			let _request = request;
			panic!("simulated preparation worker failure");
		})
		.join()
		.is_err()
	);
	assert!(!builder.has_preparation());
	fs::write(
		source.join("apps/game/src/main.rs"),
		"changed after preview",
	)
	.unwrap();
	let request = builder.begin_generation(&plan.id).unwrap();
	assert!(Builder::prepare_generation(request).is_err());
	assert!(!builder.has_preparation());
	assert!(builder.runs.is_empty());
	assert!(!root.join("output").exists());
}

#[test]
fn filesystem_preparation_admission_is_shared_by_preview_and_generation() {
	let (root, source) = fixture("preparation-admission");
	let mut builder = Builder::new(&root);
	let plan = builder
		.preview(input(&source, &root.join("output")))
		.unwrap();
	let preview = builder
		.begin_preview(input(&source, &root.join("other")))
		.unwrap();
	assert!(
		builder
			.begin_preview(input(&source, &root.join("third")))
			.is_err()
	);
	assert!(builder.begin_generation(&plan.id).is_err());
	drop(preview);
	let generation = builder.begin_generation(&plan.id).unwrap();
	assert!(
		builder
			.begin_preview(input(&source, &root.join("other")))
			.is_err()
	);
	drop(generation);
	let invalid = builder.begin_preview(input(&source, &source)).unwrap();
	assert!(Builder::prepare_preview(invalid).is_err());
	let generation = builder.begin_generation(&plan.id).unwrap();
	drop(generation);
	let request = builder
		.begin_preview(input(&source, &root.join("other")))
		.unwrap();
	let prepared = Builder::prepare_preview(request).unwrap();
	// The prepared payload reserves capacity until it is installed or dropped.
	assert!(builder.begin_generation(&plan.id).is_err());
	builder.finish_preview(prepared).unwrap();
	drop(builder.begin_generation(&plan.id).unwrap());
	assert!(!builder.has_preparation());
}

#[test]
fn invalidated_preparations_hold_admission_until_discarded_and_cannot_publish() {
	let (root, source) = fixture("stale-preparations");
	let mut builder = Builder::new(&root);
	let request = builder
		.begin_preview(input(&source, &root.join("old")))
		.unwrap();
	let prepared = Builder::prepare_preview(request).unwrap();
	builder.clear_previews();
	assert!(builder.has_preparation());
	assert!(
		builder
			.begin_preview(input(&source, &root.join("current")))
			.is_err()
	);
	assert!(builder.finish_preview(prepared).is_err());
	assert!(!builder.has_preparation());
	let current = builder
		.begin_preview(input(&source, &root.join("current")))
		.unwrap();
	assert!(builder.has_preparation());
	let plan = builder
		.finish_preview(Builder::prepare_preview(current).unwrap())
		.unwrap();
	let request = builder.begin_generation(&plan.id).unwrap();
	let old = Builder::prepare_generation(request).unwrap();
	builder.clear_previews();
	assert!(builder.has_preparation());
	assert!(
		builder
			.begin_preview(input(&source, &root.join("next")))
			.is_err()
	);
	let mut jobs = Jobs::new(&root);
	assert!(builder.finish_generation(old, &mut jobs).is_err());
	assert!(!builder.has_preparation());
	assert!(jobs.runs.is_empty());
	assert!(builder.runs.is_empty());
	let next_plan = builder.preview(input(&source, &root.join("next"))).unwrap();
	let next = builder.begin_generation(&next_plan.id).unwrap();
	builder.clear_previews();
	assert!(builder.has_preparation());
	assert!(
		builder
			.begin_preview(input(&source, &root.join("retry")))
			.is_err()
	);
	drop(next);
	assert!(!builder.has_preparation());
	drop(
		builder
			.begin_preview(input(&source, &root.join("retry")))
			.unwrap(),
	);
}

#[test]
fn destination_created_during_preparation_prevents_queueing_and_keeps_staging() {
	let (root, source) = fixture("prepare-destination");
	let mut builder = Builder::new(&root);
	let destination = root.join("output");
	let plan = builder.preview(input(&source, &destination)).unwrap();
	let request = builder.begin_generation(&plan.id).unwrap();
	let prepared = Builder::prepare_generation(request).unwrap();
	let staging = prepared.run.staging.clone();
	fs::create_dir(&destination).unwrap();
	fs::write(destination.join("owned.txt"), "keep").unwrap();
	let mut jobs = Jobs::new(&root);
	assert!(builder.finish_generation(prepared, &mut jobs).is_err());
	assert!(!builder.has_preparation());
	assert!(jobs.runs.is_empty());
	assert!(builder.runs.is_empty());
	assert!(staging.join("apps/game/src/main.rs").exists());
	assert_eq!(
		fs::read_to_string(destination.join("owned.txt")).unwrap(),
		"keep"
	);
}

#[test]
fn preparation_leaves_service_available_and_blocks_workspace_reconfiguration() {
	use std::sync::{Arc, Barrier, Mutex};
	let (root, source) = fixture("service-preparation");
	let data = root.join("admin");
	fs::create_dir(&data).unwrap();
	let service = Arc::new(Mutex::new(crate::AdminService::new(
		source.clone(),
		root.join("logs"),
		data,
	)));
	let request = service
		.lock()
		.unwrap()
		.builder
		.begin_preview(input(&source, &root.join("output")))
		.unwrap();
	let barrier = Arc::new(Barrier::new(2));
	let worker_barrier = Arc::clone(&barrier);
	let worker = std::thread::spawn(move || {
		worker_barrier.wait();
		Builder::prepare_preview(request)
	});
	{
		let mut state = service
			.try_lock()
			.expect("preparation must not hold service lock");
		let original_workspace = state.workspace.clone();
		assert!(
			state
				.configure(
					source.to_str().unwrap(),
					root.join("other-logs").to_str().unwrap()
				)
				.is_err()
		);
		assert_eq!(state.workspace, original_workspace);
		state.snapshot();
		state.tick();
	}
	barrier.wait();
	let prepared = worker.join().unwrap().unwrap();
	let mut state = service.lock().unwrap();
	state.builder.finish_preview(prepared).unwrap();
	assert!(!state.builder.has_preparation());
}

#[test]
fn template_preview_includes_valid_scene_and_prefab_json_only_under_game_assets() {
	let (root, source) = fixture("asset-json");
	fs::create_dir(source.join("apps/game/assets/prefabs")).unwrap();
	fs::write(
		source.join("apps/game/assets/scene.json"),
		r#"{"schema_version":3,"entities":[],"lights":[]}"#,
	)
	.unwrap();
	fs::write(
		source.join("apps/game/assets/prefabs/robot.json"),
		r#"{"schema_version":1,"entities":[]}"#,
	)
	.unwrap();
	fs::write(source.join("crates/runtime/src/private.json"), "{}").unwrap();
	let mut builder = Builder::new(&root.join("admin"));
	let plan = builder
		.preview(input(&source, &root.join("output")))
		.unwrap();
	let paths = plan
		.files
		.iter()
		.map(|f| f.path.as_str())
		.collect::<Vec<_>>();
	assert!(paths.contains(&"apps/game/assets/scene.json"));
	assert!(paths.contains(&"apps/game/assets/prefabs/robot.json"));
	assert!(!paths.iter().any(|p| p.ends_with("private.json")));
	assert!(!root.join("output").exists());
	fs::remove_dir_all(root).unwrap();
}

#[test]
fn template_preview_rejects_malformed_and_oversized_asset_json_without_publishing() {
	let (root, source) = fixture("bad-asset-json");
	let path = source.join("apps/game/assets/scene.json");
	let mut builder = Builder::new(&root.join("admin"));
	fs::write(&path, "{malformed").unwrap();
	assert!(
		builder
			.preview(input(&source, &root.join("output")))
			.is_err()
	);
	let file = fs::File::create(&path).unwrap();
	file.set_len(16 * 1024 * 1024 + 1).unwrap();
	drop(file);
	let error = builder
		.preview(input(&source, &root.join("output")))
		.unwrap_err();
	assert!(error.to_string().contains("16 MiB"), "{error}");
	assert!(!root.join("output").exists());
	fs::remove_dir_all(root).unwrap();
}
#[test]
fn generated_workspace_builds_without_source_and_links_history() {
	let (root, source) = fixture("independent");
	let admin = root.join("admin");
	let mut builder = Builder::new(&admin);
	let mut jobs = Jobs::new(&admin);
	let mut processes = Processes::new(&admin);
	let plan = builder
		.preview(input(&source, &root.join("output")))
		.unwrap();
	let generated = builder.generate(&plan.id, &mut jobs).unwrap();
	fs::rename(&source, root.join("source-unavailable")).unwrap();
	for _ in 0..400 {
		processes.poll();
		for mut entry in processes.output() {
			jobs.enrich(&mut entry);
		}
		jobs.tick(&mut processes, &root.join("logs"));
		builder.tick(&mut jobs);
		if builder.runs[0].state != "VALIDATING" {
			break;
		}
		std::thread::sleep(std::time::Duration::from_millis(25));
	}
	if !cfg!(windows) {
		assert_eq!(builder.runs[0].state, "FAILED");
		assert_eq!(
			builder.runs[0].detail.as_deref(),
			Some("Project publication currently supports Windows only")
		);
		assert!(generated.staging.join("generation-manifest.json").is_file());
		assert!(!root.join("output").exists());
		assert_eq!(Builder::new(&admin).runs[0].state, "FAILED");
		return;
	}
	assert_eq!(
		builder.runs[0].state, "SUCCEEDED",
		"{:?}",
		builder.runs[0].detail
	);
	assert!(root.join("output/generation-manifest.json").is_file());
	assert!(!generated.staging.exists());
	assert!(!root.join("output/apps/admin-dashboard").exists());
	let manifest: serde_json::Value =
		serde_json::from_slice(&fs::read(root.join("output/generation-manifest.json")).unwrap())
			.unwrap();
	assert_eq!(manifest["operation_id"], generated.job_id);
	assert!(!manifest["artifacts"].as_array().unwrap().is_empty());
	assert!(
		jobs.runs[0]
			.artifacts
			.iter()
			.all(|path| Path::new(path).is_file())
	);
	let mut service = crate::AdminService::new(source.clone(), root.join("logs"), admin.clone());
	let mut diagnostic = crate::tests::record("game-fixture", 1);
	diagnostic.file = Some("apps/game/src/main.rs".into());
	diagnostic.line = Some(1);
	diagnostic.correlation.operation_id = Some(generated.job_id.clone());
	service.collector.ingest(diagnostic);
	let cursor = service
		.collector
		.page(0, 10, &crate::collector::LogFilter::default())
		.entries[0]
		.cursor;
	assert_eq!(
		service.source(cursor).unwrap().0,
		root.join("output/apps/game/src/main.rs")
			.canonicalize()
			.unwrap()
	);
	let mut board = crate::board::Board::new(&admin);
	let card = board
		.save(
			None,
			crate::board::Draft {
				title: "Review client build".into(),
				description: String::new(),
				priority: "Normal".into(),
				component: "game".into(),
				column: crate::board::Column::Backlog,
				session_id: None,
			},
		)
		.unwrap();
	board.link(&card.id, &generated.job_id).unwrap();
	assert_eq!(board.cards[0].column, crate::board::Column::Backlog);
	assert_eq!(Builder::new(&admin).runs[0].state, "SUCCEEDED");
}
#[test]
fn cancellation_keeps_destination_absent() {
	let (root, source) = fixture("cancel");
	let mut builder = Builder::new(&root.join("admin"));
	let mut jobs = Jobs::new(&root.join("admin"));
	let mut processes = Processes::new(&root.join("admin"));
	let plan = builder
		.preview(input(&source, &root.join("output")))
		.unwrap();
	let run = builder.generate(&plan.id, &mut jobs).unwrap();
	jobs.cancel(&run.job_id, &mut processes).unwrap();
	builder.tick(&mut jobs);
	assert_eq!(builder.runs[0].state, "CANCELLED");
	assert!(!root.join("output").exists());
	assert!(run.staging.exists());
}
#[test]
fn destination_created_during_validation_is_preserved() {
	let (root, source) = fixture("collision");
	let mut builder = Builder::new(&root.join("admin"));
	let mut jobs = Jobs::new(&root.join("admin"));
	let plan = builder
		.preview(input(&source, &root.join("output")))
		.unwrap();
	builder.generate(&plan.id, &mut jobs).unwrap();
	fs::create_dir(root.join("output")).unwrap();
	fs::write(root.join("output/user.txt"), "keep").unwrap();
	jobs.runs[0].state = Status::Succeeded;
	builder.tick(&mut jobs);
	assert_eq!(builder.runs[0].state, "FAILED");
	assert_eq!(
		fs::read_to_string(root.join("output/user.txt")).unwrap(),
		"keep"
	);
}
#[test]
#[ignore = "builds the full copied native game with Vulkan SDK"]
fn real_native_template_validation() {
	let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../..")
		.canonicalize()
		.unwrap();
	// This test copies the real workspace, so its destination must be outside
	// that source. Sandboxed runners can provide a writable external location.
	let output_root = std::env::var_os("ENGINE_QA_TEMPLATE_ROOT")
		.map(PathBuf::from)
		.unwrap_or_else(std::env::temp_dir);
	let root = output_root.join(format!(
		"engine-native-smoke-{}-{}",
		std::process::id(),
		now_ms()
	));
	fs::create_dir(&root).unwrap();
	let admin = root.join("admin");
	let mut builder = Builder::new(&admin);
	let mut jobs = Jobs::new(&admin);
	let mut processes = Processes::new(&admin);
	let plan = builder
		.preview(input(&source, &root.join("client")))
		.unwrap();
	builder.generate(&plan.id, &mut jobs).unwrap();
	for _ in 0..2400 {
		processes.poll();
		for mut entry in processes.output() {
			if entry.message.contains("error:") {
				eprintln!("{}", entry.message);
			}
			jobs.enrich(&mut entry);
		}
		jobs.tick(&mut processes, &root.join("logs"));
		builder.tick(&mut jobs);
		if builder.runs[0].state != "VALIDATING" {
			break;
		}
		std::thread::sleep(std::time::Duration::from_millis(100));
	}
	assert_eq!(
		builder.runs[0].state, "SUCCEEDED",
		"{:?}",
		builder.runs[0].detail
	);
	let manifest: serde_json::Value =
		serde_json::from_slice(&fs::read(root.join("client/generation-manifest.json")).unwrap())
			.unwrap();
	assert!(manifest["engine_revision"].as_str().is_some());
	assert!(!manifest["artifacts"].as_array().unwrap().is_empty());
	assert!(
		jobs.runs[0]
			.artifacts
			.iter()
			.all(|path| Path::new(path).is_file())
	);
	let main = fs::read_to_string(root.join("client/apps/game/src/main.rs")).unwrap();
	assert!(!main.contains("PathBuf::from(env!(\"CARGO_MANIFEST_DIR\")).join(\"assets\")"));
	let cargo = fs::read_to_string(root.join("client/Cargo.toml")).unwrap();
	assert!(!cargo.contains("admin-dashboard"));
	eprintln!(
		"Validated native client at {}",
		root.join("client").display()
	);
}
#[test]
fn error_to_card_failed_job_rerun_and_generated_client() {
	let (root, source) = fixture("workflow");
	let admin = root.join("admin");
	let logs = root.join("logs");
	fs::create_dir_all(&logs).unwrap();
	fs::write(source.join("Cargo.toml"), "[workspace]\nresolver='2'\nmembers=['apps/game','crates/logger','crates/renderer','crates/runtime','crates/bridge']\n").unwrap();
	let mut service = crate::AdminService::new(source.clone(), logs, admin.clone());
	let mut diagnostic = crate::tests::record("game-workflow", 1);
	diagnostic.level = engine_logger::Level::Error;
	diagnostic.message = "renderer initialization failed".into();
	service.collector.ingest(diagnostic.clone());
	let card = service
		.board
		.save(
			None,
			crate::board::Draft {
				title: diagnostic.message,
				description: "Investigate error and verify repair".into(),
				priority: "High".into(),
				component: "renderer".into(),
				column: crate::board::Column::InProgress,
				session_id: Some(diagnostic.session_id),
			},
		)
		.unwrap();
	for (source_text, expected) in [
		("fn main(){let value:u32=\"wrong\";}", Status::Failed),
		("fn main(){}", Status::Succeeded),
	] {
		fs::write(source.join("apps/game/src/main.rs"), source_text).unwrap();
		let job = service
			.jobs
			.queue(&source, Kind::Build, "game".into(), "dev".into())
			.unwrap();
		service.jobs.set_offline(&job.id, true);
		service.board.link(&card.id, &job.id).unwrap();
		for _ in 0..400 {
			service.tick();
			if !matches!(service.jobs.runs[0].state, Status::Queued | Status::Running) {
				break;
			}
			std::thread::sleep(std::time::Duration::from_millis(25));
		}
		assert_eq!(service.jobs.runs[0].state, expected);
		if expected == Status::Failed {
			let page = service.collector.page(
				0,
				100,
				&crate::collector::LogFilter {
					operation_id: Some(job.id.clone()),
					severity: Some(engine_logger::Level::Error),
					..Default::default()
				},
			);
			assert!(!page.entries.is_empty());
		}
	}
	assert_eq!(
		service.board.cards[0].column,
		crate::board::Column::InProgress
	);
	assert_eq!(service.board.cards[0].jobs.len(), 2);
	let plan = service
		.builder
		.preview(input(&source, &root.join("client")))
		.unwrap();
	let generated = service
		.builder
		.generate(&plan.id, &mut service.jobs)
		.unwrap();
	for _ in 0..400 {
		service.tick();
		if service.builder.runs[0].state != "VALIDATING" {
			break;
		}
		std::thread::sleep(std::time::Duration::from_millis(25));
	}
	if cfg!(windows) {
		assert_eq!(service.builder.runs[0].state, "SUCCEEDED");
	} else {
		assert_eq!(service.builder.runs[0].state, "FAILED");
		assert_eq!(
			service.builder.runs[0].detail.as_deref(),
			Some("Project publication currently supports Windows only")
		);
		assert!(generated.staging.join("generation-manifest.json").is_file());
		assert!(!root.join("client").exists());
	}
	service.board.link(&card.id, &generated.job_id).unwrap();
	let restored = crate::board::Board::new(&admin);
	assert_eq!(restored.cards[0].jobs.len(), 3);
	assert_eq!(restored.cards[0].column, crate::board::Column::InProgress);
}
