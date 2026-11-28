use super::*;
use std::{fs, sync::mpsc, time::Duration};

const SCENE: &[u8] = include_bytes!("../../../../apps/game/assets/scenes/camera-room.json");
const WAIT: Duration = Duration::from_secs(4);

fn fixture(root: &Path, bytes: &[u8]) -> PrefetchedScene {
	PrefetchedScene {
		asset_root: root.to_path_buf(),
		digest: digest(bytes),
		parsed: parse(bytes).unwrap(),
		estimated_bytes: 100,
	}
}

fn key(root: &Path, scene: &str, source_generation: u64) -> PrefetchKey {
	PrefetchKey {
		asset_root: root.into(),
		scene: SceneId::new(scene).unwrap(),
		source_generation,
	}
}

fn queue_when_ready(worker: &ScenePrefetch, key: PrefetchKey, path: PathBuf) {
	let deadline = std::time::Instant::now() + WAIT;
	while !worker.queue(key.clone(), path.clone()) {
		assert!(
			std::time::Instant::now() < deadline,
			"prefetch queue remained contended"
		);
		std::thread::yield_now();
	}
}

fn wait_ready(worker: &ScenePrefetch) {
	let state = worker.mailbox.state.lock().unwrap();
	let (state, _) = worker
		.mailbox
		.changed
		.wait_timeout_while(state, WAIT, |state| state.ready.is_none())
		.unwrap();
	assert!(
		state.ready.is_some(),
		"prefetch worker did not return a document"
	);
}

#[test]
fn newer_generation_cancels_an_in_flight_document_and_claims_once() {
	let (started_tx, started_rx) = mpsc::channel();
	let (release_tx, release_rx) = mpsc::channel();
	let worker = ScenePrefetch::spawn(move |job| {
		if job.key.source_generation == 1 {
			started_tx.send(()).unwrap();
			release_rx.recv_timeout(WAIT).unwrap();
			assert!(job.cancelled.load(Ordering::Relaxed));
		}
		Some(fixture(&job.key.asset_root, SCENE))
	})
	.unwrap();
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	let first = key(root, "camera-room", 1);
	let second = key(root, "camera-room", 2);
	queue_when_ready(&worker, first, PathBuf::from("first.json"));
	started_rx.recv_timeout(WAIT).unwrap();
	queue_when_ready(&worker, second.clone(), PathBuf::from("second.json"));
	release_tx.send(()).unwrap();
	wait_ready(&worker);
	assert!(worker.take(&second).is_some());
	assert!(worker.take(&second).is_none());
}

#[test]
fn fresh_bytes_are_compared_before_a_warm_result_is_used() {
	let dir = std::env::temp_dir().join(format!("scene-prefetch-test-{}", uuid::Uuid::new_v4()));
	fs::create_dir(&dir).unwrap();
	let dir = dir.canonicalize().unwrap();
	let path = dir.join("scene.json");
	let result = (|| {
		fs::write(&path, SCENE)?;
		let (_, hit, estimate) = fixture(&dir, SCENE).load_or_refresh(&path).unwrap();
		assert_eq!(hit, PrefetchOutcome::Hit);
		assert_eq!(estimate, 100);
		let mut changed = SCENE.to_vec();
		changed.push(b'\n');
		fs::write(&path, &changed)?;
		assert_eq!(
			fixture(&dir, SCENE).load_or_refresh(&path).unwrap().1,
			PrefetchOutcome::Stale
		);
		fs::write(&path, b"not JSON")?;
		assert!(fixture(&dir, SCENE).load_or_refresh(&path).is_err());
		let mut large_scene = SCENE.to_vec();
		large_scene.resize(MAX_PREFETCH_SOURCE_BYTES + 1, b' ');
		fs::write(&path, large_scene)?;
		assert!(read_bounded(&File::open(&path)?)?.is_none());
		let (parsed, freshness, _) = fixture(&dir, SCENE).load_or_refresh(&path).unwrap();
		assert_eq!(freshness, PrefetchOutcome::Stale);
		assert!(!parsed.data.entities.is_empty());
		Ok::<(), io::Error>(())
	})();
	fs::remove_dir_all(dir).unwrap();
	result.unwrap();
}

#[test]
fn confirmed_requests_report_failure_then_read_a_large_scene_on_retry() {
	let dir = std::env::temp_dir().join(format!("confirmed-scene-{}", uuid::Uuid::new_v4()));
	fs::create_dir(&dir).unwrap();
	let path = dir.join("scene.json");
	fs::write(&path, b"not a scene").unwrap();
	let worker = ScenePrefetch::new().unwrap();
	let request = key(&dir.canonicalize().unwrap(), "scene", 1);

	let read = || {
		let deadline = std::time::Instant::now() + WAIT;
		while !worker.confirm(request.clone(), PathBuf::from("scene.json")) {
			assert!(std::time::Instant::now() < deadline);
			std::thread::yield_now();
		}
		wait_ready(&worker);
		worker
			.take_confirmed(&request)
			.expect("confirmed terminal outcome")
	};

	assert!(read().is_err());
	let mut bytes = SCENE.to_vec();
	bytes.resize(MAX_PREFETCH_SOURCE_BYTES + 1, b' ');
	fs::write(&path, bytes).unwrap();
	let scene = read().unwrap();
	assert_eq!(scene.outcome, PrefetchOutcome::Miss);
	assert!(!scene.document.data.entities.is_empty());
	assert!(worker.take_confirmed(&request).is_none());
	fs::remove_dir_all(dir).unwrap();
}
