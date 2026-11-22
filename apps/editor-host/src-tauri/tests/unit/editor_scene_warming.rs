use super::*;
use std::{sync::mpsc, time::Duration};

const WAIT: Duration = Duration::from_secs(5);

fn document() -> SceneData {
	SceneData::from_json(include_str!(
		"../../../../game/assets/scenes/courtyard-release-learning.json"
	))
	.unwrap()
}

fn key(scene: &str, revision: u64) -> WarmKey {
	WarmKey {
		workspace: 1,
		revision,
		scene: SceneId::new(scene).unwrap(),
		asset_generation: 0,
		origin: SceneId::new("courtyard").unwrap(),
	}
}

fn wait_ready(worker: &WarmWorker) {
	let state = worker.mailbox.state.lock().unwrap();
	let (state, _) = worker
		.mailbox
		.wake
		.wait_timeout_while(state, WAIT, |state| state.ready.is_none())
		.unwrap();
	assert!(state.ready.is_some());
}

#[test]
fn replaced_warm_jobs_cannot_publish_and_a_matching_document_is_claimed_once() {
	let (started, signal) = mpsc::channel();
	let (release, wait) = mpsc::channel();
	let worker = WarmWorker::spawn(false, move |job| {
		if job.key.revision == 1 {
			started.send(()).unwrap();
			wait.recv_timeout(WAIT).unwrap();
			assert!(job.cancel.load(Ordering::Relaxed));
		}
		Some(WarmedScene {
			references: PreparedSceneReferences::build(Arc::clone(&job.document), || false)
				.unwrap()?,
			models: None,
		})
	})
	.unwrap();
	let old = Arc::new(document());
	let new = Arc::new(document());
	worker.request(key("garden", 1), old, PathBuf::new());
	signal.recv_timeout(WAIT).unwrap();
	worker.request(key("other", 2), Arc::clone(&new), PathBuf::new());
	worker.request(key("garden", 3), Arc::clone(&new), PathBuf::new());
	release.send(()).unwrap();
	wait_ready(&worker);
	let mut warming = SceneWarming {
		worker: Some(worker),
		asset_generation: 0,
	};
	assert!(matches!(
		warming.claim(1, &key("garden", 3).scene, &new),
		WarmClaim::Hit(_)
	));
	assert!(matches!(
		warming.claim(1, &key("garden", 3).scene, &new),
		WarmClaim::Unrequested
	));
	warming.invalidate_assets();
	assert!(!warming.is_requested());
}

#[test]
fn a_different_retained_draft_cannot_reuse_the_previous_reference_list() {
	let worker = WarmWorker::new().unwrap();
	let scene = key("garden", 1);
	let original = Arc::new(document());
	worker.request(scene.clone(), Arc::clone(&original), PathBuf::new());
	wait_ready(&worker);
	let warming = SceneWarming {
		worker: Some(worker),
		asset_generation: 0,
	};
	assert!(matches!(
		warming.claim(1, &scene.scene, &document()),
		WarmClaim::Stale
	));
}
