use super::*;
use std::{sync::mpsc, time::Duration};

const SCENE: &[u8] =
	include_bytes!("../../../../apps/game/assets/scenes/courtyard-release-learning.json");
const WAIT: Duration = Duration::from_secs(4);

fn fixture(bytes: &[u8]) -> PrefetchedScene {
	PrefetchedScene {
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
		Some(fixture(SCENE))
	})
		.unwrap();
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	let first = key(root, "courtyard", 1);
	let second = key(root, "courtyard", 2);
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
	let dir = std::env::temp_dir().join(format!("courtyard-prefetch-test-{}", std::process::id()));
	fs::create_dir(&dir).unwrap();
	let path = dir.join("scene.json");
	let result = (|| {
		fs::write(&path, SCENE)?;
		let (_, hit, estimate) = fixture(SCENE).load_or_refresh(&path).unwrap();
		assert_eq!(hit, PrefetchOutcome::Hit);
		assert_eq!(estimate, 100);
		let mut changed = SCENE.to_vec();
		changed.push(b'\n');
		fs::write(&path, &changed)?;
		assert_eq!(
			fixture(SCENE).load_or_refresh(&path).unwrap().1,
			PrefetchOutcome::Stale
		);
		fs::write(&path, b"not JSON")?;
		assert!(fixture(SCENE).load_or_refresh(&path).is_err());
		fs::write(&path, vec![0; MAX_PREFETCH_SOURCE_BYTES + 1])?;
		assert!(read_bounded(&path)?.is_none());
		Ok::<(), io::Error>(())
	})();
	fs::remove_dir_all(dir).unwrap();
	result.unwrap();
}
