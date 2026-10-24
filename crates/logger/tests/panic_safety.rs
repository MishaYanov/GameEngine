use std::{
	fmt,
	sync::{
		Arc,
		atomic::{AtomicUsize, Ordering},
	},
};
struct PanicsDuringFormatting;
impl fmt::Display for PanicsDuringFormatting {
	fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
		panic!("formatting failed")
	}
}
#[test]
fn recursive_panic_capture_does_not_deadlock_and_recovers() {
	let calls = Arc::new(AtomicUsize::new(0));
	let previous_calls = calls.clone();
	std::panic::set_hook(Box::new(move |_| {
		previous_calls.fetch_add(1, Ordering::Relaxed);
	}));
	let mut config = engine_logger::Config::new("panic-safety");
	config.console = false;
	let _guard = engine_logger::init(config).unwrap();
	assert!(
		std::panic::catch_unwind(|| engine_logger::info!("{}", PanicsDuringFormatting)).is_err()
	);
	assert_eq!(calls.load(Ordering::Relaxed), 1);
	let health = engine_logger::snapshot(Default::default()).health;
	assert_eq!(health.dropped_memory, 1);
	engine_logger::info!("collector recovered");
	assert_eq!(
		engine_logger::snapshot(Default::default()).entries[0].message,
		"collector recovered"
	);
}
