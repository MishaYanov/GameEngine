use super::*;

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
fn template_preview_includes_game_assets_and_excludes_historical_demos() {
	let (root, source) = fixture("asset-json");
	let assets = source.join("projects/cinder-oath/assets");
	fs::create_dir(assets.join("prefabs")).unwrap();
	fs::write(
		assets.join("scene.json"),
		r#"{"schema_version":3,"entities":[],"lights":[]}"#,
	)
	.unwrap();
	fs::write(
		assets.join("prefabs/robot.json"),
		r#"{"schema_version":1,"entities":[]}"#,
	)
	.unwrap();
	fs::write(assets.join("soundtrack.wav"), b"fixture").unwrap();
	fs::create_dir_all(source.join("apps/game/assets")).unwrap();
	fs::write(source.join("apps/game/assets/legacy.json"), "{}").unwrap();
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
	assert!(paths.contains(&"projects/cinder-oath/assets/scene.json"));
	assert!(paths.contains(&"projects/cinder-oath/assets/prefabs/robot.json"));
	assert!(paths.contains(&"projects/cinder-oath/assets/soundtrack.wav"));
	assert!(!paths.iter().any(|p| p.ends_with("legacy.json")));
	assert!(!paths.iter().any(|p| p.ends_with("private.json")));
	assert!(!root.join("output").exists());
	fs::remove_dir_all(root).unwrap();
}

#[test]
fn template_preview_rejects_malformed_and_oversized_asset_json_without_publishing() {
	let (root, source) = fixture("bad-asset-json");
	let path = source.join("projects/cinder-oath/assets/scene.json");
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
