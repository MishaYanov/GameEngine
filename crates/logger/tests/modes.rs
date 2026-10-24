use engine_logger::{Level, read_jsonl};
use std::{fs, process::Command, time::SystemTime};

#[test]
fn application_modes_and_error_file() {
	// Each child owns a fresh global logger and its environment; no unsafe set_var.
	if std::env::var_os("LOGGER_MODE_TEST_CHILD").is_some() {
		if std::env::var("ENGINE_LOG_MODE").as_deref() == Ok("invalid") {
			let error = engine_logger::init_application("mode-test").err().unwrap();
			assert!(error.to_string().contains("ENGINE_LOG_MODE"));
			return;
		}
		std::panic::set_hook(Box::new(|_| {}));
		let guard = engine_logger::init_application("mode-test").unwrap();
		engine_logger::info!("regular marker");
		engine_logger::warn!("warning marker");
		engine_logger::debug!("debug marker");
		engine_logger::trace!("trace marker");
		engine_logger::error!("error marker");
		assert!(std::panic::catch_unwind(|| panic!("panic marker")).is_err());
		drop(guard);
		let health = engine_logger::snapshot(Default::default()).health;
		assert!(health.persistence_error.is_none());
		assert_eq!(health.dropped_persistence, 0);
		return;
	}
	let root = std::env::temp_dir().join(format!(
		"engine-modes-{}-{}",
		std::process::id(),
		SystemTime::now()
			.duration_since(SystemTime::UNIX_EPOCH)
			.unwrap()
			.as_nanos()
	));
	for mode in [None, Some("regular"), Some("debug"), Some("invalid")] {
		for console in ["0", "1"] {
			let directory = root.join(format!("{}-{console}", mode.unwrap_or("default")));
			let mut child = Command::new(std::env::current_exe().unwrap());
			child
				.args(["--exact", "application_modes_and_error_file", "--nocapture"])
				.env("LOGGER_MODE_TEST_CHILD", "1")
				.env("ENGINE_LOG_DIR", &directory)
				.env("ENGINE_LOG_CONSOLE", console)
				.env_remove("ENGINE_LOG_MODE");
			if let Some(mode) = mode {
				child.env("ENGINE_LOG_MODE", mode);
			}
			let output = child.output().unwrap();
			assert!(
				output.status.success(),
				"{}\n{}",
				String::from_utf8_lossy(&output.stdout),
				String::from_utf8_lossy(&output.stderr)
			);
			if mode == Some("invalid") {
				assert!(!directory.exists());
				continue;
			}
			let session = fs::read_dir(&directory)
				.unwrap()
				.next()
				.unwrap()
				.unwrap()
				.path();
			let ordinary = read_jsonl(session.join("000000.jsonl"), 512 * 1024).unwrap();
			let errors = read_jsonl(session.join("errors.jsonl"), 512 * 1024).unwrap();
			assert_eq!(
				ordinary.iter().any(|e| e.level == Level::Debug),
				mode == Some("debug")
			);
			assert!(!ordinary.iter().any(|e| e.level == Level::Trace));
			assert!(ordinary.iter().any(|e| e.level == Level::Info));
			assert!(ordinary.iter().any(|e| e.level == Level::Warning));
			assert_eq!(errors.len(), 2);
			assert!(errors.iter().all(|e| e.level == Level::Error));
			assert!(errors.iter().any(|e| e.component == "panic"));
			assert_eq!(
				errors,
				ordinary
					.into_iter()
					.filter(|e| e.level == Level::Error)
					.collect::<Vec<_>>()
			);
			let stderr = String::from_utf8_lossy(&output.stderr);
			assert_eq!(stderr.contains("regular marker"), console == "1");
			assert_eq!(
				stderr.contains("debug marker"),
				console == "1" && mode == Some("debug")
			);
		}
	}
	fs::remove_dir_all(root).unwrap();
}
