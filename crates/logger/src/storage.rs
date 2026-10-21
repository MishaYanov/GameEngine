use crate::{LogEntry, LoggerHealth};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufWriter, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, SyncSender, TrySendError},
    },
    time::{Duration, Instant},
};

#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub directory: PathBuf,
    pub queue_capacity: usize,
    pub max_file_bytes: usize,
    pub max_record_bytes: usize,
    /// Segment count per stream (ordinary and error), each bounded by max_file_bytes.
    pub max_files: usize,
    /// Inactive session retention. Active sessions are never deleted.
    pub max_sessions: usize,
}
impl StorageConfig {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
            queue_capacity: 1024,
            max_file_bytes: 4 * 1024 * 1024,
            max_record_bytes: 512 * 1024,
            max_files: 4,
            max_sessions: 10,
        }
    }
    pub(crate) fn validate(&self) -> io::Result<()> {
        if self.queue_capacity == 0
            || self.max_files == 0
            || self.max_sessions == 0
            || self.max_record_bytes < 2
            || self.max_record_bytes > self.max_file_bytes
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "storage limits must be positive and record limit must fit a file",
            ));
        }
        Ok(())
    }
}
#[derive(Default)]
pub(crate) struct Health {
    error: Mutex<Option<String>>,
    disk_drops: AtomicU64,
    memory_drops: AtomicU64,
    stopped: AtomicBool,
}
impl Health {
    fn error(&self, error: impl ToString) {
        *self.error.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(crate::bounded(format_args!("{}", error.to_string()), 4096));
    }
    pub(crate) fn drop_memory(&self) {
        self.memory_drops.fetch_add(1, Ordering::Relaxed);
    }
    pub(crate) fn snapshot(&self) -> LoggerHealth {
        LoggerHealth {
            persistence_error: self.error.lock().unwrap_or_else(|e| e.into_inner()).clone(),
            dropped_persistence: self.disk_drops.load(Ordering::Relaxed),
            dropped_memory: self.memory_drops.load(Ordering::Relaxed),
            writer_stopped: self.stopped.load(Ordering::Acquire),
        }
    }
}
enum Command {
    Entry(Box<LogEntry>),
    Flush(mpsc::Sender<()>),
}
pub(crate) struct Writer {
    sender: Mutex<Option<SyncSender<Command>>>,
    health: Arc<Health>,
    completed: Arc<(Mutex<bool>, Condvar)>,
}
impl Writer {
    pub(crate) fn start(
        config: StorageConfig,
        session: String,
        health: Arc<Health>,
    ) -> io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(config.queue_capacity);
        let completed = Arc::new((Mutex::new(false), Condvar::new()));
        // Storage failures degrade persistence, not the application or memory collector.
        let mut sink = match Sink::new(config.clone(), &session, &health) {
            Ok(sink) => Some(sink),
            Err(error) => {
                health.error(error);
                None
            }
        };
        let worker_health = health.clone();
        let worker_completed = completed.clone();
        std::thread::Builder::new()
            .name("engine-log-writer".into())
            .spawn(move || {
                loop {
                    match receiver.recv_timeout(Duration::from_millis(100)) {
                        Ok(Command::Entry(entry)) => {
                            let Some(active_sink) = sink.as_mut() else {
                                worker_health.disk_drops.fetch_add(1, Ordering::Relaxed);
                                continue;
                            };
                            let result = active_sink.write(&entry);
                            if let Err(error) = result {
                                worker_health.error(error);
                                worker_health.disk_drops.fetch_add(1, Ordering::Relaxed);
                                // Sink disables only the failed stream; the other keeps working.
                            }
                        }
                        Ok(Command::Flush(ack)) => {
                            if let Some(sink) = sink.as_mut()
                                && let Err(error) = sink.flush()
                            {
                                worker_health.error(error);
                            }
                            let _ = ack.send(());
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            if let Some(sink) = sink.as_mut()
                                && let Err(error) = sink.flush()
                            {
                                worker_health.error(error);
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
                if let Some(sink) = sink.as_mut()
                    && let Err(error) = sink.flush()
                {
                    worker_health.error(error);
                }
                drop(sink);
                worker_health.stopped.store(true, Ordering::Release);
                let (lock, condition) = &*worker_completed;
                *lock.lock().unwrap_or_else(|e| e.into_inner()) = true;
                condition.notify_all();
            })?;
        Ok(Self {
            sender: Mutex::new(Some(sender)),
            health,
            completed,
        })
    }
    pub(crate) fn enqueue(&self, entry: &LogEntry) {
        // Try-lock is also used by panic reporting; never wait on the sender mutex.
        let sent = self
            .sender
            .try_lock()
            .ok()
            .and_then(|sender| {
                sender
                    .as_ref()
                    .map(|sender| sender.try_send(Command::Entry(Box::new(entry.clone()))))
            })
            .is_some_and(|result| result.is_ok());
        if !sent {
            self.health.disk_drops.fetch_add(1, Ordering::Relaxed);
        }
    }
    pub(crate) fn flush(&self, timeout: Duration) -> bool {
        let start = Instant::now();
        let (ack, response) = mpsc::channel();
        let mut command = Command::Flush(ack);
        loop {
            let result = match self.sender.try_lock() {
                Ok(sender) => match sender.as_ref() {
                    Some(sender) => sender.try_send(command),
                    None => return false,
                },
                Err(_) => {
                    if start.elapsed() >= timeout {
                        return false;
                    }
                    std::thread::yield_now();
                    continue;
                }
            };
            match result {
                Ok(()) => break,
                Err(TrySendError::Disconnected(_)) => return false,
                Err(TrySendError::Full(returned)) => command = returned,
            }
            if start.elapsed() >= timeout {
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        response
            .recv_timeout(timeout.saturating_sub(start.elapsed()))
            .is_ok()
    }
    pub(crate) fn shutdown(&self, timeout: Duration) -> bool {
        // Producer lock is only held around non-blocking sends.
        self.sender.lock().unwrap_or_else(|e| e.into_inner()).take();
        let (lock, condition) = &*self.completed;
        let done = lock.lock().unwrap_or_else(|e| e.into_inner());
        let (done, _) = condition
            .wait_timeout_while(done, timeout, |done| !*done)
            .unwrap_or_else(|e| e.into_inner());
        *done
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        self.shutdown(Duration::from_secs(2));
    }
}

struct Sink {
    ordinary: Option<Segment>,
    errors: Option<Segment>,
    _lock: File,
}
impl Sink {
    fn new(config: StorageConfig, session: &str, health: &Health) -> io::Result<Self> {
        fs::create_dir_all(&config.directory)?;
        let directory = config.directory.join(session);
        fs::create_dir(&directory)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(directory.join("session.lock"))?;
        lock.try_lock()
            .map_err(|e| io::Error::other(e.to_string()))?;
        let open = |errors| match Segment::new(config.clone(), directory.clone(), errors) {
            Ok(segment) => Some(segment),
            Err(error) => {
                health.error(error);
                None
            }
        };
        let ordinary = open(false);
        let errors = open(true);
        if let Err(error) = prune_sessions(&config, session) {
            health.error(error);
        }
        Ok(Self {
            ordinary,
            errors,
            _lock: lock,
        })
    }
    fn write(&mut self, entry: &LogEntry) -> io::Result<()> {
        let ordinary = Self::write_stream(&mut self.ordinary, entry);
        let errors = if entry.level == crate::Level::Error {
            Self::write_stream(&mut self.errors, entry)
        } else {
            Ok(())
        };
        ordinary.and(errors)
    }
    fn write_stream(stream: &mut Option<Segment>, entry: &LogEntry) -> io::Result<()> {
        let result = stream
            .as_mut()
            .ok_or_else(|| io::Error::other("log stream unavailable"))?
            .write(entry);
        if result.is_err() {
            // A partial write can leave a fragment. Never append to a damaged stream.
            stream.take();
        }
        result
    }
    fn flush(&mut self) -> io::Result<()> {
        let ordinary = Self::flush_stream(&mut self.ordinary);
        let errors = Self::flush_stream(&mut self.errors);
        ordinary.and(errors)
    }
    fn flush_stream(stream: &mut Option<Segment>) -> io::Result<()> {
        let result = stream.as_mut().map_or(Ok(()), Segment::flush);
        if result.is_err() {
            stream.take();
        }
        result
    }
}
struct Segment {
    config: StorageConfig,
    directory: PathBuf,
    file: Option<BufWriter<File>>,
    bytes: usize,
    index: u64,
    paths: std::collections::VecDeque<PathBuf>,
    errors: bool,
}
impl Segment {
    fn name(errors: bool, index: u64) -> String {
        match (errors, index) {
            (false, _) => format!("{index:06}.jsonl"),
            (true, 0) => "errors.jsonl".into(),
            (true, _) => format!("errors-{index:06}.jsonl"),
        }
    }
    fn new(config: StorageConfig, directory: PathBuf, errors: bool) -> io::Result<Self> {
        let file_path = directory.join(Self::name(errors, 0));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file_path)?;
        Ok(Self {
            config,
            directory,
            errors,
            file: Some(BufWriter::with_capacity(64 * 1024, file)),
            bytes: 0,
            index: 0,
            paths: [file_path].into(),
        })
    }
    fn write(&mut self, entry: &LogEntry) -> io::Result<()> {
        // Serialization occurs only on the background thread and is size bounded.
        let mut output = ByteLimit {
            bytes: Vec::new(),
            limit: self.config.max_record_bytes - 1,
        };
        serde_json::to_writer(&mut output, entry)?;
        output.bytes.push(b'\n');
        if self.bytes + output.bytes.len() > self.config.max_file_bytes {
            self.flush()?;
            // Remove before creation so even a failed rotation cannot grow beyond the limit.
            while self.paths.len() >= self.config.max_files {
                let old = self.paths.front().unwrap();
                if *old == self.directory.join(Self::name(self.errors, self.index)) {
                    // Close before removing on Windows, retaining unique segment names for readers.
                    self.file.take();
                }
                fs::remove_file(old)?;
                self.paths.pop_front();
            }
            let next = self.index + 1;
            let path = self.directory.join(Self::name(self.errors, next));
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            self.file = Some(BufWriter::with_capacity(64 * 1024, file));
            self.index = next;
            self.bytes = 0;
            self.paths.push_back(path);
        }
        self.file
            .as_mut()
            .ok_or_else(|| io::Error::other("log segment unavailable"))?
            .write_all(&output.bytes)?;
        self.bytes += output.bytes.len();
        if entry.level == crate::Level::Error {
            self.flush()?;
        }
        Ok(())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file
            .as_mut()
            .ok_or_else(|| io::Error::other("log segment unavailable"))?
            .flush()
    }
}
struct ByteLimit {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for ByteLimit {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("log record exceeds max_record_bytes"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn prune_sessions(config: &StorageConfig, current: &str) -> io::Result<()> {
    let application = current.rsplitn(3, '-').last().unwrap_or(current);
    let prefix = format!("{application}-");
    let mut sessions = vec![];
    for entry in fs::read_dir(&config.directory)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !entry.file_type()?.is_dir() || !name.starts_with(&prefix) {
            continue;
        }
        let suffix = &name[prefix.len()..];
        let Some((pid, timestamp)) = suffix.split_once('-') else {
            continue;
        };
        if pid.parse::<u32>().is_err() {
            continue;
        }
        let Ok(timestamp) = timestamp.parse::<u128>() else {
            continue;
        };
        sessions.push((timestamp, entry.path(), name));
    }
    sessions.sort_by_key(|s| s.0);
    let remove_count = sessions.len().saturating_sub(config.max_sessions);
    let mut removed = 0;
    for (_, path, name) in sessions {
        if removed >= remove_count {
            break;
        }
        if name == current {
            continue;
        }
        let lock_path = path.join("session.lock");
        let Ok(lock) = OpenOptions::new().read(true).write(true).open(&lock_path) else {
            continue;
        };
        if lock.try_lock().is_err() {
            continue;
        }
        // Only owned segment files are removed. Do not follow symlinks or recursively delete directories.
        for entry in fs::read_dir(&path)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let stem = name.strip_suffix(".jsonl");
            let numbered = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
            let owned = stem.is_some_and(|s| {
                numbered(s) || s == "errors" || s.strip_prefix("errors-").is_some_and(numbered)
            });
            if entry.file_type()?.is_file() && owned {
                fs::remove_file(entry.path())?;
            }
        }
        drop(lock);
        fs::remove_file(lock_path)?;
        fs::remove_dir(&path)?;
        removed += 1;
    }
    Ok(())
}
/// Read complete records only; an incomplete trailing line is ignored after an abrupt exit.
/// Malformed complete records or unsupported schema versions return an error.
pub fn read_jsonl(path: impl AsRef<Path>, max_line_bytes: usize) -> io::Result<Vec<LogEntry>> {
    let mut reader = io::BufReader::new(File::open(path)?);
    let mut entries = vec![];
    loop {
        let mut line = Vec::new();
        // take bounds allocation even when the input has no newline.
        use std::io::Read;
        let read = reader
            .by_ref()
            .take(max_line_bytes as u64 + 1)
            .read_until(b'\n', &mut line)?;
        if read == 0 {
            break;
        }
        if line.len() > max_line_bytes {
            return Err(io::Error::other("JSONL line exceeds configured limit"));
        }
        if !line.ends_with(b"\n") {
            break;
        }
        let entry: LogEntry = serde_json::from_slice(&line)?;
        if entry.schema_version != crate::SCHEMA_VERSION {
            return Err(io::Error::other("unsupported log schema version"));
        }
        entries.push(entry);
    }
    Ok(entries)
}
#[cfg(test)]
#[path = "tests_storage.rs"]
mod tests;
