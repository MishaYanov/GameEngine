use engine_core::{
	asset_jobs::{
		AssetJobContext, AssetJobError, AssetJobLimits, AssetJobResult, AssetJobs,
		AssetSubmitError, CpuAssetImporter, ImportedAsset,
	},
	asset_registry::{AssetMetadata, AssetRef},
};
use std::{
	sync::mpsc::{self, Receiver, Sender},
	time::{Duration, Instant},
};

struct GateImporter {
	entered: Sender<String>,
	release: Receiver<()>,
	bytes: usize,
}
impl CpuAssetImporter for GateImporter {
	type Output = String;
	fn import(
		&self,
		reference: &AssetRef,
		context: &AssetJobContext,
	) -> Result<ImportedAsset<String>, String> {
		self.entered
			.send(reference.path.clone())
			.map_err(|e| e.to_string())?;
		self.release
			.recv_timeout(Duration::from_secs(3))
			.map_err(|e| e.to_string())?;
		context.check_cancelled()?;
		Ok(ImportedAsset {
			value: reference.path.clone(),
			metadata: AssetMetadata {
				estimated_bytes: self.bytes as u64,
				..Default::default()
			},
			bytes: self.bytes,
		})
	}
}
fn gate(limits: AssetJobLimits, bytes: usize) -> (AssetJobs<String>, Receiver<String>, Sender<()>) {
	let (entered, started) = mpsc::channel();
	let (release, waiting) = mpsc::channel();
	(
		AssetJobs::new(
			GateImporter {
				entered,
				release: waiting,
				bytes,
			},
			limits,
		)
		.unwrap(),
		started,
		release,
	)
}
fn reference(path: &str) -> AssetRef {
	AssetRef::new(path, None).unwrap()
}
fn drain<T: Send + 'static>(jobs: &mut AssetJobs<T>) -> AssetJobResult<T> {
	let deadline = Instant::now() + Duration::from_secs(3);
	loop {
		if let Some(result) = jobs.try_drain() {
			return result;
		}
		assert!(
			Instant::now() < deadline,
			"asset worker did not produce a result"
		);
		std::thread::sleep(Duration::from_millis(1));
	}
}

#[test]
fn pending_count_includes_undrained_results_and_reservations_release_only_on_drain() {
	let limits = AssetJobLimits {
		max_pending_jobs: 2,
		max_reserved_bytes: 12,
		max_job_bytes: 8,
	};
	let (mut jobs, entered, release) = gate(limits, 4);
	let first = jobs.try_submit(reference("a.glb"), 8).unwrap();
	entered.recv_timeout(Duration::from_secs(3)).unwrap();
	let second = jobs.try_submit(reference("b.glb"), 4).unwrap();
	assert_ne!(first, second);
	assert_eq!(
		jobs.try_submit(reference("c.glb"), 1),
		Err(AssetSubmitError::QueueFull)
	);
	let stats = jobs.try_stats().unwrap();
	assert_eq!((stats.pending_jobs, stats.reserved_bytes), (2, 12));
	release.send(()).unwrap();
	entered.recv_timeout(Duration::from_secs(3)).unwrap();
	assert_eq!(
		jobs.try_stats().unwrap().pending_jobs,
		2,
		"completion alone must not free retained payload reservations"
	);
	let result = drain(&mut jobs);
	assert_eq!(result.id, first);
	assert_eq!(result.result.unwrap().value, "a.glb");
	assert_eq!(jobs.try_stats().unwrap().reserved_bytes, 4);
	release.send(()).unwrap();
	assert_eq!(drain(&mut jobs).id, second);
	assert_eq!(jobs.try_stats().unwrap().reserved_bytes, 0);
}

#[test]
fn queued_and_completed_cancellation_do_not_leak_results_or_release_budget_early() {
	let (mut jobs, entered, release) = gate(
		AssetJobLimits {
			max_pending_jobs: 2,
			max_reserved_bytes: 8,
			max_job_bytes: 4,
		},
		4,
	);
	let first = jobs.try_submit(reference("a.glb"), 4).unwrap();
	entered.recv_timeout(Duration::from_secs(3)).unwrap();
	let second = jobs.try_submit(reference("b.glb"), 4).unwrap();
	assert!(jobs.cancel(second).unwrap());
	assert_eq!(jobs.try_stats().unwrap().reserved_bytes, 8);
	release.send(()).unwrap();
	assert_eq!(drain(&mut jobs).id, first);
	let cancelled = drain(&mut jobs);
	assert_eq!(cancelled.id, second);
	assert!(matches!(cancelled.result, Err(AssetJobError::Cancelled)));
	assert!(!jobs.cancel(second).unwrap());
	assert_eq!(jobs.try_stats().unwrap().pending_jobs, 0);
}

#[test]
fn cancelling_a_completed_undrained_result_prevents_payload_delivery() {
	let (mut jobs, entered, release) = gate(
		AssetJobLimits {
			max_pending_jobs: 2,
			max_reserved_bytes: 8,
			max_job_bytes: 4,
		},
		4,
	);
	let first = jobs.try_submit(reference("a.glb"), 4).unwrap();
	entered.recv_timeout(Duration::from_secs(3)).unwrap();
	let second = jobs.try_submit(reference("b.glb"), 4).unwrap();
	release.send(()).unwrap();
	// Worker entry into the next request proves the first result is already queued.
	entered.recv_timeout(Duration::from_secs(3)).unwrap();
	assert!(jobs.cancel(first).unwrap());
	let cancelled = drain(&mut jobs);
	assert_eq!(cancelled.id, first);
	assert!(matches!(cancelled.result, Err(AssetJobError::Cancelled)));
	release.send(()).unwrap();
	let accepted = drain(&mut jobs);
	assert_eq!(accepted.id, second);
	assert!(accepted.result.is_ok());
	assert_eq!(jobs.try_stats().unwrap().reserved_bytes, 0);
}

#[test]
fn generation_switch_discards_old_work_and_retains_its_reservation_until_drain() {
	let (mut jobs, entered, release) = gate(
		AssetJobLimits {
			max_pending_jobs: 2,
			max_reserved_bytes: 8,
			max_job_bytes: 4,
		},
		4,
	);
	jobs.try_submit(reference("old.glb"), 4).unwrap();
	entered.recv_timeout(Duration::from_secs(3)).unwrap();
	let old = jobs.generation();
	assert!(jobs.advance_generation().unwrap() > old);
	let current = jobs.try_submit(reference("new.glb"), 4).unwrap();
	assert_eq!(jobs.try_stats().unwrap().reserved_bytes, 8);
	release.send(()).unwrap();
	entered.recv_timeout(Duration::from_secs(3)).unwrap();
	release.send(()).unwrap();
	let accepted = drain(&mut jobs);
	assert_eq!(accepted.id, current);
	assert_eq!(accepted.generation, jobs.generation());
	assert_eq!(accepted.result.unwrap().value, "new.glb");
	assert_eq!(jobs.try_stats().unwrap().pending_jobs, 0);
	assert!(jobs.try_drain().is_none());
}

#[test]
fn job_byte_and_reference_budgets_reject_submission_atomically() {
	let (mut jobs, entered, release) = gate(
		AssetJobLimits {
			max_pending_jobs: 4,
			max_reserved_bytes: 8,
			max_job_bytes: 8,
		},
		1,
	);
	assert_eq!(
		jobs.try_submit(reference("a.glb"), 0),
		Err(AssetSubmitError::ByteBudget)
	);
	assert_eq!(
		jobs.try_submit(reference("a.glb"), 9),
		Err(AssetSubmitError::ByteBudget)
	);
	assert!(matches!(
		jobs.try_submit(
			AssetRef {
				path: "../outside.glb".into(),
				mesh_index: None
			},
			1
		),
		Err(AssetSubmitError::InvalidReference(_))
	));
	jobs.try_submit(reference("a.glb"), 6).unwrap();
	entered.recv_timeout(Duration::from_secs(3)).unwrap();
	assert_eq!(
		jobs.try_submit(reference("b.glb"), 3),
		Err(AssetSubmitError::ByteBudget)
	);
	assert_eq!(jobs.try_stats().unwrap().pending_jobs, 1);
	release.send(()).unwrap();
	drain(&mut jobs);
	assert_eq!(jobs.try_stats().unwrap().reserved_bytes, 0);
}

struct FaultImporter;
impl CpuAssetImporter for FaultImporter {
	type Output = ();
	fn import(
		&self,
		reference: &AssetRef,
		_context: &AssetJobContext,
	) -> Result<ImportedAsset<()>, String> {
		match reference.path.as_str() {
			"panic.glb" => panic!("controlled importer panic"),
			"error.glb" => Err("controlled import error".into()),
			"large-error.glb" => Err("界".repeat(100_000)),
			"spare-capacity.glb" => {
				let mut message = String::with_capacity(1_000_000);
				message.push_str("small error");
				Err(message)
			}
			_ => Ok(ImportedAsset {
				value: (),
				metadata: AssetMetadata::default(),
				bytes: 9,
			}),
		}
	}
}

#[test]
fn importer_errors_are_utf8_bounded_and_do_not_retain_excess_string_capacity() {
	let mut jobs = AssetJobs::new(FaultImporter, AssetJobLimits::default()).unwrap();
	for path in ["large-error.glb", "spare-capacity.glb"] {
		jobs.try_submit(reference(path), 1).unwrap();
		let result = drain(&mut jobs);
		let Err(AssetJobError::Import(message)) = result.result else {
			panic!("expected bounded import error");
		};
		assert!(!message.is_empty());
		assert!(message.len() <= engine_core::asset_jobs::MAX_ASSET_ERROR_BYTES);
		assert!(message.capacity() <= engine_core::asset_jobs::MAX_ASSET_ERROR_BYTES);
		if path == "large-error.glb" {
			assert!(message.chars().all(|character| character == '界'));
		} else {
			assert_eq!(message, "small error");
		}
		assert_eq!(jobs.try_stats().unwrap().reserved_bytes, 0);
	}
}
#[test]
fn importer_panics_errors_and_oversized_payloads_are_isolated_and_worker_remains_usable() {
	let mut jobs = AssetJobs::new(
		FaultImporter,
		AssetJobLimits {
			max_pending_jobs: 1,
			max_reserved_bytes: 8,
			max_job_bytes: 8,
		},
	)
	.unwrap();
	for path in ["panic.glb", "error.glb", "oversized.glb"] {
		jobs.try_submit(reference(path), 8).unwrap();
		let result = drain(&mut jobs);
		match path {
			"panic.glb" | "error.glb" => {
				assert!(matches!(result.result, Err(AssetJobError::Import(_))))
			}
			_ => assert!(matches!(
				result.result,
				Err(AssetJobError::ByteLimit { bytes: 9, limit: 8 })
			)),
		}
		assert_eq!(jobs.try_stats().unwrap().pending_jobs, 0);
		assert!(!jobs.try_stats().unwrap().worker_finished);
	}
}

#[test]
fn dropping_broker_does_not_wait_for_an_in_flight_import() {
	let (jobs, entered, release) = gate(AssetJobLimits::default(), 1);
	jobs.try_submit(reference("slow.glb"), 1).unwrap();
	entered.recv_timeout(Duration::from_secs(3)).unwrap();
	let (dropped, finished) = mpsc::channel();
	let dropper = std::thread::spawn(move || {
		drop(jobs);
		dropped.send(()).unwrap();
	});
	let released_without_wait = finished.recv_timeout(Duration::from_secs(1));
	release.send(()).unwrap();
	dropper.join().unwrap();
	assert!(
		released_without_wait.is_ok(),
		"broker destruction waited for importer completion"
	);
}

#[test]
fn job_ids_from_another_broker_cannot_cancel_a_local_import() {
	let (mut first, first_entered, first_release) = gate(AssetJobLimits::default(), 1);
	let (mut second, second_entered, second_release) = gate(AssetJobLimits::default(), 1);
	let foreign = first.try_submit(reference("first.glb"), 1).unwrap();
	first_entered.recv_timeout(Duration::from_secs(3)).unwrap();
	let local = second.try_submit(reference("second.glb"), 1).unwrap();
	second_entered.recv_timeout(Duration::from_secs(3)).unwrap();
	let cancelled = second.cancel(foreign).unwrap();
	// Release both mocks even when the isolation assertion will fail.
	first_release.send(()).unwrap();
	second_release.send(()).unwrap();
	drain(&mut first);
	let local_result = drain(&mut second);
	assert!(!cancelled, "a foreign broker job id cancelled local work");
	assert_eq!(local_result.id, local);
	assert!(local_result.result.is_ok());
}
