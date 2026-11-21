use super::Processes;
use crate::now_ms;
use engine_logger::{Level, LogEntry};
use std::{
	io::{self, BufRead, Read},
	process::Child,
	sync::{
		Arc,
		atomic::{AtomicUsize, Ordering},
	},
};

const MAX_LINE_BYTES: u64 = 8192;

pub(super) struct Output {
	pending: std::sync::Arc<std::sync::atomic::AtomicUsize>,
	run_id: String,
	application: String,
	line: String,
	error: bool,
}

impl Processes {
	pub(super) fn start_output_readers(
		&self,
		child: &mut Child,
		id: &str,
		run_id: &str,
		pending: &Arc<AtomicUsize>,
	) {
		for (stream, error) in [
			(
				child
					.stdout
					.take()
					.map(|s| Box::new(s) as Box<dyn Read + Send>),
				false,
			),
			(
				child
					.stderr
					.take()
					.map(|s| Box::new(s) as Box<dyn Read + Send>),
				true,
			),
		] {
			if let Some(stream) = stream {
				let pending = pending.clone();
				pending.fetch_add(1, Ordering::SeqCst);
				let sender = self.sender.clone();
				let run_id = run_id.to_owned();
				let application = id.to_owned();
				let drops = self.output_drops.clone();
				std::thread::spawn(move || {
					let mut reader = io::BufReader::new(stream);
					loop {
						let mut line = vec![];
						match reader
							.by_ref()
							.take(MAX_LINE_BYTES)
							.read_until(b'\n', &mut line)
						{
							Ok(0) | Err(_) => break,
							Ok(_) => {
								let line = String::from_utf8_lossy(&line).trim_end().to_owned();
								pending.fetch_add(1, Ordering::SeqCst);
								if sender
									.try_send(Output {
										pending: pending.clone(),
										run_id: run_id.clone(),
										application: application.clone(),
										line,
										error,
									})
									.is_err()
								{
									pending.fetch_sub(1, Ordering::SeqCst);
									drops.fetch_add(1, Ordering::Relaxed);
								}
							}
						}
					}
					pending.fetch_sub(1, Ordering::SeqCst);
				});
			}
		}
	}

	pub fn output(&mut self) -> Vec<LogEntry> {
		let mut records = vec![];
		while let Ok(output) = self.receiver.try_recv() {
			output.pending.fetch_sub(1, Ordering::SeqCst);
			self.sequence += 1;
			records.push(LogEntry {
				schema_version: engine_logger::SCHEMA_VERSION,
				session_id: self.session.clone(),
				sequence: self.sequence,
				timestamp_ms: now_ms(),
				application: output.application.clone(),
				component: "process-output".into(),
				process_id: self
					.history
					.iter()
					.find(|r| r.id == output.run_id)
					.map_or(0, |r| r.pid),
				thread: "admin-output-reader".into(),
				level: Level::Info,
				message: output.line,
				file: None,
				line: None,
				module: None,
				error: None,
				stack_trace: None,
				correlation: engine_logger::Correlation {
					operation_id: Some(if output.application.starts_with("job-") {
						output.application.clone()
					} else {
						output.run_id
					}),
					..Default::default()
				},
				attributes: [(
					"stream".into(),
					if output.error { "stderr" } else { "stdout" }.into(),
				)]
				.into(),
			});
		}
		records
	}
	pub fn output_complete(&self, id: &str) -> bool {
		self.drains
			.get(id)
			.is_none_or(|pending| pending.load(Ordering::SeqCst) == 0)
	}
}
