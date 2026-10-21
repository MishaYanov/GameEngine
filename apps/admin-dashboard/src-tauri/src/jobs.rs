use crate::{
    now_ms,
    processes::{ProcessState, Processes},
    save_json,
};
use engine_logger::{Level, LogEntry};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum Kind {
    Build,
    Test,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum Status {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub kind: Kind,
    pub package: String,
    pub profile: String,
    pub workspace: PathBuf,
    pub state: Status,
    pub queued_ms: u64,
    pub started_ms: Option<u64>,
    pub ended_ms: Option<u64>,
    pub process_id: Option<String>,
    pub exit_code: Option<i32>,
    pub toolchain: Option<String>,
    pub artifacts: Vec<String>,
    #[serde(default)]
    pub offline: bool,
    pub detail: Option<String>,
}
pub const PACKAGES: &[&str] = &[
    "game",
    "app",
    "compiler",
    "runtime",
    "renderer",
    "bridge",
    "engine-logger",
    "engine-admin",
];
pub struct Jobs {
    pub runs: Vec<Run>,
    path: PathBuf,
    pub storage_error: Option<String>,
    sequence: u64,
}
impl Jobs {
    pub fn new(data: &Path) -> Self {
        let path = data.join("jobs.json");
        let result = File::open(&path).and_then(|file| {
            serde_json::from_reader::<_, Vec<Run>>(file.take(4 * 1024 * 1024))
                .map_err(io::Error::other)
        });
        let (mut runs, storage_error) = match result {
            Ok(r) => (r, None),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (vec![], None),
            Err(e) => (vec![], Some(e.to_string())),
        };
        runs.truncate(200);
        for run in &mut runs {
            if matches!(run.state, Status::Running | Status::Queued) {
                run.state = Status::Interrupted;
                run.ended_ms = Some(now_ms());
                run.detail = Some("Dashboard restarted; job was not resumed".into());
            }
        }
        let mut jobs = Self {
            runs,
            path,
            storage_error,
            sequence: 0,
        };
        jobs.persist();
        jobs
    }
    fn persist(&mut self) {
        self.storage_error = save_json(&self.path, &self.runs)
            .err()
            .map(|e| e.to_string());
    }
    pub fn queue(
        &mut self,
        workspace: &Path,
        kind: Kind,
        package: String,
        profile: String,
    ) -> io::Result<Run> {
        if !PACKAGES.contains(&package.as_str()) || !["dev", "release"].contains(&profile.as_str())
        {
            return Err(io::Error::other("Unsupported Cargo package or profile"));
        }
        if self
            .runs
            .iter()
            .filter(|r| matches!(r.state, Status::Queued | Status::Running))
            .count()
            >= 50
        {
            return Err(io::Error::other("Job queue is full (50 jobs)"));
        }
        self.sequence += 1;
        let run = Run {
            id: format!("job-{}-{}-{}", std::process::id(), now_ms(), self.sequence),
            kind,
            package,
            profile,
            workspace: workspace.to_owned(),
            state: Status::Queued,
            queued_ms: now_ms(),
            started_ms: None,
            ended_ms: None,
            process_id: None,
            exit_code: None,
            toolchain: None,
            artifacts: vec![],
            offline: false,
            detail: None,
        };
        self.runs.insert(0, run.clone());
        self.runs.truncate(200);
        self.persist();
        Ok(run)
    }
    pub fn set_offline(&mut self, id: &str, offline: bool) {
        if let Some(run) = self.runs.iter_mut().find(|r| r.id == id) {
            run.offline = offline;
        }
        self.persist();
    }
    pub fn interrupt(&mut self) {
        for run in &mut self.runs {
            if matches!(run.state, Status::Queued | Status::Running) {
                run.state = Status::Interrupted;
                run.ended_ms = Some(now_ms());
                run.detail = Some("Dashboard closed before completion".into());
            }
        }
        self.persist();
    }
    pub fn cancel(&mut self, id: &str, processes: &mut Processes) -> io::Result<()> {
        let run = self
            .runs
            .iter_mut()
            .find(|r| r.id == id)
            .ok_or_else(|| io::Error::other("Unknown job"))?;
        if !matches!(run.state, Status::Queued | Status::Running) {
            return Err(io::Error::other("Job has already finished"));
        }
        if run.state == Status::Running {
            processes.stop(&run.id)?;
        }
        run.state = Status::Cancelled;
        run.ended_ms = Some(now_ms());
        self.persist();
        Ok(())
    }
    pub fn tick(&mut self, processes: &mut Processes, logs: &Path) {
        let history = processes.history();
        let mut changed = false;
        for run in &mut self.runs {
            if run.state == Status::Running
                && let Some(process) = history
                    .iter()
                    .find(|p| Some(&p.id) == run.process_id.as_ref())
                && process.state != ProcessState::Running
                && processes.output_complete(&process.id)
            {
                run.state = match process.state {
                    ProcessState::Succeeded => Status::Succeeded,
                    ProcessState::Stopped => Status::Cancelled,
                    ProcessState::Interrupted => Status::Interrupted,
                    _ => Status::Failed,
                };
                run.exit_code = process.exit_code;
                run.ended_ms = process.ended_ms;
                changed = true;
            }
        }
        if !self.runs.iter().any(|r| r.state == Status::Running)
            && let Some(run) = self
                .runs
                .iter_mut()
                .rev()
                .find(|r| r.state == Status::Queued)
        {
            let mut command = Command::new("cargo");
            command.args([
                if run.kind == Kind::Build {
                    "build"
                } else {
                    "test"
                },
                "-p",
                &run.package,
                "--profile",
                &run.profile,
                "--message-format=json",
            ]);
            if run.offline {
                command.arg("--offline");
            }
            let mut version = Command::new("cargo");
            version.arg("--version");
            crate::processes::hidden(&mut version);
            run.toolchain = version
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());
            run.started_ms = Some(now_ms());
            match processes.start_command(&run.id, command, &run.workspace, logs) {
                Ok(process) => {
                    run.process_id = Some(process.id);
                    run.state = Status::Running;
                }
                Err(e) => {
                    run.state = Status::Failed;
                    run.detail = Some(e.to_string());
                    run.ended_ms = Some(now_ms());
                }
            }
            changed = true;
        }
        if changed {
            self.persist();
        }
    }
    pub fn relocate_artifacts(&mut self, id: &str, from: &Path, to: &Path) {
        let mut changed = false;
        if let Some(run) = self.runs.iter_mut().find(|r| r.id == id) {
            for artifact in &mut run.artifacts {
                if let Some(relative) = crate::relative_to(Path::new(artifact), from) {
                    *artifact = to.join(relative).to_string_lossy().into_owned();
                    changed = true;
                }
            }
        }
        if changed {
            self.persist();
        }
    }
    pub fn enrich(&mut self, entry: &mut LogEntry) {
        let Some(run) = self
            .runs
            .iter_mut()
            .find(|r| Some(&r.id) == entry.correlation.operation_id.as_ref())
        else {
            return;
        };
        if run.kind == Kind::Build {
            entry.correlation.build_id = Some(run.id.clone());
        } else {
            entry.correlation.test_run_id = Some(run.id.clone());
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&entry.message) else {
            return;
        };
        match value["reason"].as_str() {
            Some("compiler-message") => {
                let message = &value["message"];
                entry.component = "cargo-diagnostic".into();
                entry.level = match message["level"].as_str() {
                    Some("error" | "failure-note") => Level::Error,
                    Some("warning") => Level::Warning,
                    _ => Level::Info,
                };
                entry.message = message["rendered"]
                    .as_str()
                    .or(message["message"].as_str())
                    .unwrap_or("Cargo diagnostic")
                    .to_owned();
                if let Some(span) = message["spans"]
                    .as_array()
                    .and_then(|spans| spans.iter().find(|s| s["is_primary"] == true))
                {
                    entry.file = span["file_name"].as_str().map(str::to_owned);
                    entry.line = span["line_start"]
                        .as_u64()
                        .and_then(|n| u32::try_from(n).ok());
                }
            }
            Some("compiler-artifact") => {
                if value["target"]["name"]
                    .as_str()
                    .is_none_or(|name| name != run.package && name != run.package.replace('-', "_"))
                {
                    return;
                }
                if let Some(files) = value["filenames"].as_array() {
                    for file in files.iter().filter_map(|v| v.as_str()) {
                        if run.artifacts.len() < 128 && !run.artifacts.iter().any(|v| v == file) {
                            run.artifacts.push(file.into());
                        }
                    }
                }
                self.persist();
            }
            _ => {}
        }
    }
}
#[cfg(test)]
#[path = "tests_jobs.rs"]
mod tests;
