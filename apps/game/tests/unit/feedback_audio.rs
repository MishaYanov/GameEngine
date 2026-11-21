use super::*;

#[test]
fn legacy_clip_is_valid_and_only_gate_has_a_fallback() {
	let player = FeedbackAudio::new(None, false);
	assert!(player.clip(FeedbackCue::GateOpened).is_some());
	assert!(player.clip(FeedbackCue::KeyCollected).is_none());
	assert!(player.clip(FeedbackCue::RunCompleted).is_none());
}

#[test]
fn committed_facts_emit_each_edge_once_in_order() {
	let before = Facts {
		key: false,
		gate: false,
		complete: false,
	};
	let after = Facts {
		key: true,
		gate: true,
		complete: true,
	};
	assert_eq!(
		before.rising(after).collect::<Vec<_>>(),
		[
			FeedbackCue::KeyCollected,
			FeedbackCue::GateOpened,
			FeedbackCue::RunCompleted
		]
	);
	assert_eq!(after.rising(after).count(), 0);
	assert_eq!(after.rising(before).count(), 0);
}

#[cfg(windows)]
#[test]
fn a_full_queue_never_blocks_and_invalidates_previous_run() {
	let (output, input) = sync_channel(1);
	let worker = Worker {
		output,
		epoch: Arc::new(AtomicU64::new(0)),
	};
	let mut player = FeedbackAudio::default();
	player.worker = Some(worker);
	let clip = CueClip::from_wave(GATE_TONE.to_vec()).unwrap();
	let worker = player.worker.as_ref().unwrap();
	worker.enqueue(clip.clone());
	worker.enqueue(clip.clone());
	player.invalidate();
	let (queued_epoch, _) = input.try_recv().unwrap();
	assert_ne!(queued_epoch, worker.epoch.load(Ordering::Relaxed));
	assert!(input.try_recv().is_err());
	worker.enqueue(clip);
	let (queued_epoch, _) = input.try_recv().unwrap();
	assert_eq!(queued_epoch, worker.epoch.load(Ordering::Relaxed));
}
