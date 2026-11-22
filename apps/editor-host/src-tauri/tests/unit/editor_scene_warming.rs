use super::*;
use std::{sync::mpsc, time::Duration};

const WAIT: Duration = Duration::from_secs(5);

fn document() -> SceneData {
	SceneData::from_json(include_str!(
		"../../../../game/assets/scenes/camera-room.json"
	))
	.unwrap()
}

fn key(scene: &str, revision: u64) -> WarmKey {
	WarmKey {
		workspace: 1,
		revision,
		scene: SceneId::new(scene).unwrap(),
		asset_generation: 0,
		mode: SceneWarmingMode::References,
		origin: SceneId::new("camera-room").unwrap(),
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
	let worker = WarmWorker::spawn(move |job| {
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
		mode: SceneWarmingMode::References,
		..Default::default()
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
		mode: SceneWarmingMode::References,
		..Default::default()
	};
	assert!(matches!(
		warming.claim(1, &scene.scene, &document()),
		WarmClaim::Stale
	));
}

#[test]
fn changing_mode_cancels_an_inflight_job_and_retires_its_result_context() {
	let (started, signal) = mpsc::channel();
	let (release, wait) = mpsc::channel();
	let worker = WarmWorker::spawn(move |job| {
		if job.key.mode == SceneWarmingMode::References {
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
	let source = Arc::new(document());
	let mut key = key("garden", 1);
	worker.request(key.clone(), Arc::clone(&source), PathBuf::new());
	signal.recv_timeout(WAIT).unwrap();
	let mut warming = SceneWarming {
		worker: Some(worker),
		mode: SceneWarmingMode::References,
		..Default::default()
	};
	let retired_context = warming.context();
	warming.set_mode(SceneWarmingMode::Models);
	assert!(matches!(
		warming.claim(1, &key.scene, &source),
		WarmClaim::Unrequested
	));
	key.mode = SceneWarmingMode::Models;
	let worker = warming.worker.as_ref().unwrap();
	worker.request(key.clone(), Arc::clone(&source), PathBuf::new());
	release.send(()).unwrap();
	wait_ready(worker);
	assert!(matches!(
		warming.claim(1, &key.scene, &source),
		WarmClaim::Hit(_)
	));

	let result = SceneWarmingResultDto {
		scene_id: key.scene,
		scene_revision: 2,
		request_id: "request".into(),
		mode: SceneWarmingMode::Models,
		mode_revision: warming.context().mode_revision,
		references_hit: true,
		prepared_models: 1,
		adopted_models: 0,
		retained_bytes: 1024,
	};
	warming.record_presented(retired_context, completed_row(1, result.clone()));
	assert!(warming.latest("garden", 2).is_none());
	warming.record_presented(warming.context(), completed_row(1, result.clone()));
	assert_eq!(warming.latest("garden", 2), Some(&result));
	assert!(warming.latest("garden", 1).is_none());
	warming.set_mode(SceneWarmingMode::Off);
	assert!(warming.latest("garden", 2).is_none());
}

#[test]
fn warming_history_is_bounded_to_completed_presentations_in_the_workspace() {
	let mut warming = SceneWarming::default();
	let scene = key("garden", 1).scene;
	warming.cancel();
	assert!(warming.report("session".into(), 1).rows.is_empty());
	for revision in 1..=33 {
		let result = SceneWarmingResultDto {
			scene_id: scene.clone(),
			scene_revision: revision,
			request_id: format!("request-{revision}"),
			mode: SceneWarmingMode::Off,
			mode_revision: 0,
			references_hit: false,
			prepared_models: 0,
			adopted_models: 0,
			retained_bytes: 0,
		};
		warming.record_presented(warming.context(), completed_row(1, result));
	}
	let report = warming.report("session".into(), 1);
	assert_eq!(report.rows.len(), MAX_REPORT_ROWS);
	assert_eq!(report.rows.first().unwrap().result.scene_revision, 2);
	assert_eq!(report.rows.last().unwrap().result.scene_revision, 33);
	let json = serde_json::to_value(&report).unwrap();
	assert_eq!(json["session_id"], "session");
	assert_eq!(json["rows"][0]["source_scene_id"], "camera-room");
	assert_eq!(json["rows"][0]["workspace_generation"], 1);
	assert_eq!(json["rows"][0]["pre_switch_cache"]["asset_generation"], 1);
	let replacement = SceneWarming::default();
	assert!(replacement.report("session".into(), 2).rows.is_empty());
}

fn completed_row(generation: u64, result: SceneWarmingResultDto) -> WarmingSessionRowDto {
	use bridge::viewport::{
		ViewportProjection, WarmingCacheDto, WarmingCacheState, WarmingLoadIdentityDto,
		WarmingViewportDto,
	};
	WarmingSessionRowDto {
		identity: WarmingLoadIdentityDto {
			source_scene_id: Some(SceneId::new("camera-room").unwrap()),
			workspace_generation: generation,
			pre_switch_cache: WarmingCacheDto {
				state: WarmingCacheState::Retained,
				asset_generation: 1,
				loaded_models: 1,
				gpu_resources: 2,
				gpu_bytes: 1024,
			},
			viewport: WarmingViewportDto {
				width: 800,
				height: 600,
				projection: ViewportProjection::Perspective,
				grid: true,
				colliders: false,
			},
		},
		outcome: if result.references_hit {
			"hit"
		} else {
			"unrequested"
		}
			.into(),
		result,
	}
}
