use crate::{
    jobs::{Jobs, Kind, Status},
    now_ms, save_json,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    hash::{Hash, Hasher},
    io::{self, Read},
    path::{Path, PathBuf},
};
const LIMIT: u64 = 512 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Input {
    pub name: String,
    pub source: String,
    pub destination: String,
    pub profile: String,
    pub target: String,
    #[serde(default = "offline_default")]
    pub offline: bool,
}
fn offline_default() -> bool {
    true
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub bytes: u64,
    pub fingerprint: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Preview {
    pub id: String,
    pub input: Input,
    pub files: Vec<FileEntry>,
    pub total_bytes: u64,
    pub excluded: Vec<String>,
    pub template_version: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Generation {
    pub id: String,
    pub name: String,
    pub destination: PathBuf,
    pub staging: PathBuf,
    pub source: PathBuf,
    pub state: String,
    pub job_id: String,
    pub detail: Option<String>,
    pub created_ms: u64,
    pub finished_ms: Option<u64>,
    pub revision: Option<String>,
    pub profile: String,
    pub target: String,
    #[serde(default)]
    pub files: Vec<FileEntry>,
}
pub struct Builder {
    pub runs: Vec<Generation>,
    pub storage_error: Option<String>,
    path: PathBuf,
    plans: BTreeMap<String, Preview>,
    sequence: u64,
}
fn fingerprint(bytes: &[u8]) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hash);
    hash.finish()
}
fn read_file(path: &Path) -> io::Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > LIMIT {
        return Err(io::Error::other(
            "Unsupported template file or symbolic link",
        ));
    }
    let mut bytes = vec![];
    fs::File::open(path)?
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT {
        return Err(io::Error::other("Template file is too large"));
    }
    Ok(bytes)
}
fn walk(root: &Path, path: &Path, files: &mut Vec<FileEntry>, total: &mut u64) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(io::Error::other("Template symbolic links are unsupported"));
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            walk(root, &entry?.path(), files, total)?;
        }
    } else {
        let relative = path
            .strip_prefix(root)
            .map_err(io::Error::other)?
            .to_string_lossy()
            .replace('\\', "/");
        let allowed = relative.ends_with(".rs")
            || relative.ends_with("Cargo.toml")
            || relative.ends_with(".vert")
            || relative.ends_with(".frag")
            || (relative.starts_with("apps/game/assets/")
                && ["png", "jpg", "jpeg", "obj", "gltf", "glb", "bin"]
                    .contains(&path.extension().and_then(|e| e.to_str()).unwrap_or("")));
        if allowed {
            if relative.len() > 1024 {
                return Err(io::Error::other("Template path is too long"));
            }
            let bytes = read_file(path)?;
            *total += bytes.len() as u64;
            if *total > LIMIT || files.len() >= 10000 {
                return Err(io::Error::other("Template exceeds 512 MiB or 10000 files"));
            }
            files.push(FileEntry {
                path: relative,
                bytes: bytes.len() as u64,
                fingerprint: fingerprint(&bytes),
            });
        }
    }
    Ok(())
}
fn publish_new(source: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
        let destination: Vec<u16> = destination
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        // No replacement flag: a concurrent destination creator wins safely.
        if unsafe {
            windows_sys::Win32::Storage::FileSystem::MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                0,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (source, destination);
        Err(io::Error::other(
            "Project publication currently supports Windows only",
        ))
    }
}
impl Builder {
    pub fn new(data: &Path) -> Self {
        let path = data.join("projects.json");
        let loaded = fs::File::open(&path).and_then(|f| {
            serde_json::from_reader::<_, Vec<Generation>>(f.take(32 * 1024 * 1024))
                .map_err(io::Error::other)
        });
        let (mut runs, storage_error) = match loaded {
            Ok(r) => (r, None),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (vec![], None),
            Err(e) => (vec![], Some(e.to_string())),
        };
        runs.truncate(50);
        for run in &mut runs {
            if run.state == "VALIDATING" {
                run.state = "INTERRUPTED".into();
                run.detail = Some("Validation interrupted; staging retained for inspection".into());
                run.finished_ms = Some(now_ms());
            }
        }
        Self {
            runs,
            storage_error,
            path,
            plans: BTreeMap::new(),
            sequence: 0,
        }
    }
    fn persist(&mut self) {
        while self.runs.len() > 1
            && serde_json::to_vec(&self.runs).is_ok_and(|bytes| bytes.len() > 32 * 1024 * 1024)
        {
            self.runs.pop();
        }
        self.storage_error = save_json(&self.path, &self.runs)
            .err()
            .map(|e| e.to_string());
    }
    pub fn preview(&mut self, mut input: Input) -> io::Result<Preview> {
        if input.name.is_empty()
            || input.name.len() > 64
            || !input
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            || !["dev", "release"].contains(&input.profile.as_str())
            || input.target != "local"
        {
            return Err(io::Error::other(
                "Use a 1–64 character project name, dev/release profile and local target",
            ));
        }
        let source = PathBuf::from(&input.source).canonicalize()?;
        let destination = PathBuf::from(&input.destination);
        if !destination.is_absolute() || destination.file_name().is_none() || destination.exists() {
            return Err(io::Error::other(
                "Destination must be a new absolute folder",
            ));
        }
        let parent = destination
            .parent()
            .ok_or_else(|| io::Error::other("Missing destination parent"))?
            .canonicalize()?;
        let destination = parent.join(destination.file_name().unwrap());
        if destination.starts_with(&source) {
            return Err(io::Error::other(
                "Destination must be outside the source workspace",
            ));
        }
        input.source = source.to_string_lossy().into_owned();
        input.destination = destination.to_string_lossy().into_owned();
        let mut files = vec![];
        let mut total = 0;
        for path in [
            "apps/game/Cargo.toml",
            "apps/game/src",
            "apps/game/assets",
            "crates/logger/Cargo.toml",
            "crates/logger/src",
            "crates/renderer/Cargo.toml",
            "crates/renderer/build.rs",
            "crates/renderer/src",
            "crates/renderer/shaders",
            "crates/runtime/Cargo.toml",
            "crates/runtime/src",
            "crates/bridge/Cargo.toml",
            "crates/bridge/src",
        ] {
            let path = source.join(path);
            walk(&source, &path, &mut files, &mut total)?;
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        self.sequence += 1;
        let preview = Preview {
            id: format!(
                "project-{}-{}-{}",
                std::process::id(),
                now_ms(),
                self.sequence
            ),
            input,
            files,
            total_bytes: total,
            excluded: vec![
                "admin dashboard, editor UI/host, compiler".into(),
                ".git, .env, credentials, caches, target, node_modules, logs".into(),
                "unsupported asset formats and files outside template allowlist".into(),
            ],
            template_version: 1,
        };
        if self.plans.len() >= 16 {
            self.plans.clear();
        }
        self.plans.insert(preview.id.clone(), preview.clone());
        Ok(preview)
    }
    pub fn generate(&mut self, id: &str, jobs: &mut Jobs) -> io::Result<Generation> {
        if self.storage_error.is_some() {
            return Err(io::Error::other(
                "Recover project history storage before generating",
            ));
        }
        if self.runs.iter().any(|r| r.state == "VALIDATING") {
            return Err(io::Error::other("A project is already being validated"));
        }
        let plan = self
            .plans
            .get(id)
            .cloned()
            .ok_or_else(|| io::Error::other("Preview expired; preview again"))?;
        let destination = PathBuf::from(&plan.input.destination);
        if destination.exists() {
            return Err(io::Error::other("Destination is occupied"));
        }
        let source = PathBuf::from(&plan.input.source);
        let staging = destination
            .parent()
            .unwrap()
            .join(format!(".engine-{}", plan.id));
        fs::create_dir(&staging)?;
        // Retain failed staging instead of recursively deleting user-visible paths.
        let result = (|| -> io::Result<Generation> {
            for file in &plan.files {
                let path = source.join(&file.path);
                let mut ancestor = source.clone();
                for part in Path::new(&file.path).components() {
                    ancestor.push(part);
                    if fs::symlink_metadata(&ancestor)?.file_type().is_symlink() {
                        return Err(io::Error::other("Template path became a symbolic link"));
                    }
                }
                if !path.canonicalize()?.starts_with(&source) {
                    return Err(io::Error::other("Template path escaped source"));
                }
                let bytes = read_file(&path)?;
                if fingerprint(&bytes) != file.fingerprint {
                    return Err(io::Error::other(
                        "Template changed after preview; preview again",
                    ));
                }
                let output = staging.join(&file.path);
                fs::create_dir_all(output.parent().unwrap())?;
                fs::write(output, bytes)?;
            }
            fs::write(
                staging.join("Cargo.toml"),
                "[workspace]\nresolver=\"2\"\nmembers=[\"apps/game\",\"crates/logger\",\"crates/renderer\",\"crates/runtime\",\"crates/bridge\"]\n",
            )?;
            let main = staging.join("apps/game/src/main.rs");
            let text=fs::read_to_string(&main)?.replace("PathBuf::from(env!(\"CARGO_MANIFEST_DIR\")).join(\"assets\")","std::env::current_exe().expect(\"game executable path\").parent().expect(\"game executable folder\").join(\"../../apps/game/assets\")");
            fs::write(main, text)?;
            fs::write(
                staging.join("README.md"),
                format!(
                    "# {}\n\nNative game template v1. Requires Rust 1.89+ and Vulkan SDK (glslc).\n\nRun from this workspace: `cargo run -p game`.\nAssets stay under apps/game/assets; retain the workspace layout when moving output.\nRuntime logging works independently of Engine Admin.\n",
                    plan.input.name
                ),
            )?;
            let git_source = crate::comparable_path(&source)
                .to_string_lossy()
                .replace('\\', "/");
            let mut command = std::process::Command::new("git");
            command.args([
                "-c",
                &format!("safe.directory={git_source}"),
                "-C",
                &git_source,
                "rev-parse",
                "HEAD",
            ]);
            crate::processes::hidden(&mut command);
            let revision = command
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());
            let job = jobs.queue(
                &staging,
                Kind::Build,
                "game".into(),
                plan.input.profile.clone(),
            )?;
            jobs.set_offline(&job.id, plan.input.offline);
            let run = Generation {
                id: plan.id.clone(),
                name: plan.input.name.clone(),
                destination,
                staging: staging.clone(),
                source,
                state: "VALIDATING".into(),
                job_id: job.id,
                detail: None,
                created_ms: now_ms(),
                finished_ms: None,
                revision,
                profile: plan.input.profile.clone(),
                target: plan.input.target.clone(),
                files: plan.files.clone(),
            };
            Ok(run)
        })();
        match result {
            Ok(run) => {
                self.runs.insert(0, run.clone());
                self.runs.truncate(50);
                self.plans.remove(id);
                self.persist();
                Ok(run)
            }
            Err(e) => Err(io::Error::other(format!(
                "{e}; staging retained at {}",
                staging.display()
            ))),
        }
    }
    pub fn tick(&mut self, jobs: &mut Jobs) {
        let mut changed = false;
        for run in &mut self.runs {
            if run.state != "VALIDATING" {
                continue;
            }
            let Some(job) = jobs.runs.iter().find(|j| j.id == run.job_id) else {
                run.state = "INTERRUPTED".into();
                run.detail = Some("Validation job no longer retained".into());
                changed = true;
                continue;
            };
            if matches!(job.state, Status::Queued | Status::Running) {
                continue;
            }
            run.finished_ms = Some(now_ms());
            changed = true;
            if job.state != Status::Succeeded {
                run.state = match job.state {
                    Status::Cancelled => "CANCELLED",
                    Status::Interrupted => "INTERRUPTED",
                    _ => "FAILED",
                }
                .into();
                run.detail = Some("Validation did not succeed; staging retained".into());
                continue;
            }
            let artifacts: Vec<_> = job
                .artifacts
                .iter()
                .filter_map(|a| {
                    crate::relative_to(Path::new(a), &run.staging)
                        .map(|p| p.to_string_lossy().into_owned())
                })
                .collect();
            let manifest = serde_json::json!({"template_version":1,"name":run.name,"engine_revision":run.revision,"source_snapshot":"copied local working tree","profile":run.profile,"target":run.target,"operation_id":run.job_id,"toolchain":job.toolchain,"artifacts":artifacts,"files":run.files,"platform":format!("{}-{}",std::env::consts::ARCH,std::env::consts::OS),"created_ms":run.created_ms,"validated_ms":run.finished_ms});
            let result = (|| -> io::Result<()> {
                if run.destination.exists() {
                    return Err(io::Error::other(
                        "Destination became occupied; it was not overwritten",
                    ));
                }
                save_json(&run.staging.join("generation-manifest.json"), &manifest)?;
                publish_new(&run.staging, &run.destination)?;
                Ok(())
            })();
            match result {
                Ok(()) => {
                    run.state = "SUCCEEDED".into();
                    run.detail = Some("Validated independent workspace is ready".into());
                }
                Err(e) => {
                    run.state = "FAILED".into();
                    run.detail = Some(e.to_string());
                }
            }
        }
        if changed {
            self.persist();
        }
        for run in self.runs.iter().filter(|r| r.state == "SUCCEEDED") {
            jobs.relocate_artifacts(&run.job_id, &run.staging, &run.destination);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::processes::Processes;
    fn fixture(label: &str) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "builder-{label}-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let source = root.join("source");
        fs::create_dir_all(&source).unwrap();
        for (folder, name) in [
            ("apps/game", "game"),
            ("crates/logger", "engine-logger"),
            ("crates/renderer", "renderer"),
            ("crates/runtime", "runtime"),
            ("crates/bridge", "bridge"),
        ] {
            fs::create_dir_all(source.join(folder).join("src")).unwrap();
            fs::write(
                source.join(folder).join("Cargo.toml"),
                format!("[package]\nname='{name}'\nversion='0.1.0'\nedition='2021'\n"),
            )
            .unwrap();
            fs::write(
                source.join(folder).join("src").join(if name == "game" {
                    "main.rs"
                } else {
                    "lib.rs"
                }),
                if name == "game" {
                    "fn main() {}"
                } else {
                    "pub fn ready() {}"
                },
            )
            .unwrap();
        }
        fs::create_dir_all(source.join("apps/game/assets")).unwrap();
        fs::write(source.join("apps/game/assets/sample.png"), b"fixture").unwrap();
        fs::write(source.join("apps/game/assets/.env"), b"excluded").unwrap();
        fs::create_dir_all(source.join("crates/renderer/shaders")).unwrap();
        fs::write(source.join("crates/renderer/build.rs"), "fn main() {}").unwrap();
        (root, source)
    }
    fn input(source: &Path, destination: &Path) -> Input {
        Input {
            name: "fixture-game".into(),
            source: source.to_string_lossy().into_owned(),
            destination: destination.to_string_lossy().into_owned(),
            profile: "dev".into(),
            target: "local".into(),
            offline: true,
        }
    }
    #[test]
    fn preview_rejects_occupied_nested_and_changed_source() {
        let (root, source) = fixture("paths");
        let mut builder = Builder::new(&root.join("admin"));
        assert!(
            builder
                .preview(input(&source, &source.join("output")))
                .is_err()
        );
        assert!(builder.preview(input(&source, &source)).is_err());
        let plan = builder
            .preview(input(&source, &root.join("output")))
            .unwrap();
        assert!(!plan.files.iter().any(|f| f.path.ends_with(".env")));
        assert!(
            plan.files
                .iter()
                .any(|f| f.path == "crates/logger/Cargo.toml")
        );
        fs::write(source.join("apps/game/src/main.rs"), "fn main(){panic!();}").unwrap();
        let mut jobs = Jobs::new(&root.join("admin"));
        assert!(builder.generate(&plan.id, &mut jobs).is_err());
        assert!(!root.join("output").exists());
        assert!(jobs.runs.is_empty());
    }
    #[test]
    fn generated_workspace_builds_without_source_and_links_history() {
        let (root, source) = fixture("independent");
        let admin = root.join("admin");
        let mut builder = Builder::new(&admin);
        let mut jobs = Jobs::new(&admin);
        let mut processes = Processes::new(&admin);
        let plan = builder
            .preview(input(&source, &root.join("output")))
            .unwrap();
        let generated = builder.generate(&plan.id, &mut jobs).unwrap();
        fs::rename(&source, root.join("source-unavailable")).unwrap();
        for _ in 0..400 {
            processes.poll();
            for mut entry in processes.output() {
                jobs.enrich(&mut entry);
            }
            jobs.tick(&mut processes, &root.join("logs"));
            builder.tick(&mut jobs);
            if builder.runs[0].state != "VALIDATING" {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        if !cfg!(windows) {
            assert_eq!(builder.runs[0].state, "FAILED");
            assert_eq!(
                builder.runs[0].detail.as_deref(),
                Some("Project publication currently supports Windows only")
            );
            assert!(generated.staging.join("generation-manifest.json").is_file());
            assert!(!root.join("output").exists());
            assert_eq!(Builder::new(&admin).runs[0].state, "FAILED");
            return;
        }
        assert_eq!(
            builder.runs[0].state, "SUCCEEDED",
            "{:?}",
            builder.runs[0].detail
        );
        assert!(root.join("output/generation-manifest.json").is_file());
        assert!(!generated.staging.exists());
        assert!(!root.join("output/apps/admin-dashboard").exists());
        let manifest: serde_json::Value = serde_json::from_slice(
            &fs::read(root.join("output/generation-manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["operation_id"], generated.job_id);
        assert!(!manifest["artifacts"].as_array().unwrap().is_empty());
        assert!(
            jobs.runs[0]
                .artifacts
                .iter()
                .all(|path| Path::new(path).is_file())
        );
        let mut service =
            crate::AdminService::new(source.clone(), root.join("logs"), admin.clone());
        let mut diagnostic = crate::tests::record("game-fixture", 1);
        diagnostic.file = Some("apps/game/src/main.rs".into());
        diagnostic.line = Some(1);
        diagnostic.correlation.operation_id = Some(generated.job_id.clone());
        service.collector.ingest(diagnostic);
        let cursor = service
            .collector
            .page(0, 10, &crate::collector::LogFilter::default())
            .entries[0]
            .cursor;
        assert_eq!(
            service.source(cursor).unwrap().0,
            root.join("output/apps/game/src/main.rs")
                .canonicalize()
                .unwrap()
        );
        let mut board = crate::board::Board::new(&admin);
        let card = board
            .save(
                None,
                crate::board::Draft {
                    title: "Review client build".into(),
                    description: String::new(),
                    priority: "Normal".into(),
                    component: "game".into(),
                    column: crate::board::Column::Backlog,
                    session_id: None,
                },
            )
            .unwrap();
        board.link(&card.id, &generated.job_id).unwrap();
        assert_eq!(board.cards[0].column, crate::board::Column::Backlog);
        assert_eq!(Builder::new(&admin).runs[0].state, "SUCCEEDED");
    }
    #[test]
    fn cancellation_keeps_destination_absent() {
        let (root, source) = fixture("cancel");
        let mut builder = Builder::new(&root.join("admin"));
        let mut jobs = Jobs::new(&root.join("admin"));
        let mut processes = Processes::new(&root.join("admin"));
        let plan = builder
            .preview(input(&source, &root.join("output")))
            .unwrap();
        let run = builder.generate(&plan.id, &mut jobs).unwrap();
        jobs.cancel(&run.job_id, &mut processes).unwrap();
        builder.tick(&mut jobs);
        assert_eq!(builder.runs[0].state, "CANCELLED");
        assert!(!root.join("output").exists());
        assert!(run.staging.exists());
    }
    #[test]
    fn destination_created_during_validation_is_preserved() {
        let (root, source) = fixture("collision");
        let mut builder = Builder::new(&root.join("admin"));
        let mut jobs = Jobs::new(&root.join("admin"));
        let plan = builder
            .preview(input(&source, &root.join("output")))
            .unwrap();
        builder.generate(&plan.id, &mut jobs).unwrap();
        fs::create_dir(root.join("output")).unwrap();
        fs::write(root.join("output/user.txt"), "keep").unwrap();
        jobs.runs[0].state = Status::Succeeded;
        builder.tick(&mut jobs);
        assert_eq!(builder.runs[0].state, "FAILED");
        assert_eq!(
            fs::read_to_string(root.join("output/user.txt")).unwrap(),
            "keep"
        );
    }
    #[test]
    #[ignore = "builds the full copied native game with Vulkan SDK"]
    fn real_native_template_validation() {
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .unwrap();
        let root = std::env::temp_dir().join(format!("engine-native-smoke-{}", now_ms()));
        fs::create_dir_all(&root).unwrap();
        let admin = root.join("admin");
        let mut builder = Builder::new(&admin);
        let mut jobs = Jobs::new(&admin);
        let mut processes = Processes::new(&admin);
        let plan = builder
            .preview(input(&source, &root.join("client")))
            .unwrap();
        builder.generate(&plan.id, &mut jobs).unwrap();
        for _ in 0..2400 {
            processes.poll();
            for mut entry in processes.output() {
                if entry.message.contains("error:") {
                    eprintln!("{}", entry.message);
                }
                jobs.enrich(&mut entry);
            }
            jobs.tick(&mut processes, &root.join("logs"));
            builder.tick(&mut jobs);
            if builder.runs[0].state != "VALIDATING" {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert_eq!(
            builder.runs[0].state, "SUCCEEDED",
            "{:?}",
            builder.runs[0].detail
        );
        let manifest: serde_json::Value = serde_json::from_slice(
            &fs::read(root.join("client/generation-manifest.json")).unwrap(),
        )
        .unwrap();
        assert!(manifest["engine_revision"].as_str().is_some());
        assert!(!manifest["artifacts"].as_array().unwrap().is_empty());
        assert!(
            jobs.runs[0]
                .artifacts
                .iter()
                .all(|path| Path::new(path).is_file())
        );
        let main = fs::read_to_string(root.join("client/apps/game/src/main.rs")).unwrap();
        assert!(!main.contains("PathBuf::from(env!(\"CARGO_MANIFEST_DIR\")).join(\"assets\")"));
        let cargo = fs::read_to_string(root.join("client/Cargo.toml")).unwrap();
        assert!(!cargo.contains("admin-dashboard"));
        eprintln!(
            "Validated native client at {}",
            root.join("client").display()
        );
    }
    #[test]
    fn error_to_card_failed_job_rerun_and_generated_client() {
        let (root, source) = fixture("workflow");
        let admin = root.join("admin");
        let logs = root.join("logs");
        fs::create_dir_all(&logs).unwrap();
        fs::write(source.join("Cargo.toml"),"[workspace]\nresolver='2'\nmembers=['apps/game','crates/logger','crates/renderer','crates/runtime','crates/bridge']\n").unwrap();
        let mut service = crate::AdminService::new(source.clone(), logs, admin.clone());
        let mut diagnostic = crate::tests::record("game-workflow", 1);
        diagnostic.level = engine_logger::Level::Error;
        diagnostic.message = "renderer initialization failed".into();
        service.collector.ingest(diagnostic.clone());
        let card = service
            .board
            .save(
                None,
                crate::board::Draft {
                    title: diagnostic.message,
                    description: "Investigate error and verify repair".into(),
                    priority: "High".into(),
                    component: "renderer".into(),
                    column: crate::board::Column::InProgress,
                    session_id: Some(diagnostic.session_id),
                },
            )
            .unwrap();
        for (source_text, expected) in [
            ("fn main(){let value:u32=\"wrong\";}", Status::Failed),
            ("fn main(){}", Status::Succeeded),
        ] {
            fs::write(source.join("apps/game/src/main.rs"), source_text).unwrap();
            let job = service
                .jobs
                .queue(&source, Kind::Build, "game".into(), "dev".into())
                .unwrap();
            service.jobs.set_offline(&job.id, true);
            service.board.link(&card.id, &job.id).unwrap();
            for _ in 0..400 {
                service.tick();
                if !matches!(service.jobs.runs[0].state, Status::Queued | Status::Running) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            assert_eq!(service.jobs.runs[0].state, expected);
            if expected == Status::Failed {
                let page = service.collector.page(
                    0,
                    100,
                    &crate::collector::LogFilter {
                        operation_id: Some(job.id.clone()),
                        severity: Some(engine_logger::Level::Error),
                        ..Default::default()
                    },
                );
                assert!(!page.entries.is_empty());
            }
        }
        assert_eq!(
            service.board.cards[0].column,
            crate::board::Column::InProgress
        );
        assert_eq!(service.board.cards[0].jobs.len(), 2);
        let plan = service
            .builder
            .preview(input(&source, &root.join("client")))
            .unwrap();
        let generated = service
            .builder
            .generate(&plan.id, &mut service.jobs)
            .unwrap();
        for _ in 0..400 {
            service.tick();
            if service.builder.runs[0].state != "VALIDATING" {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        if cfg!(windows) {
            assert_eq!(service.builder.runs[0].state, "SUCCEEDED");
        } else {
            assert_eq!(service.builder.runs[0].state, "FAILED");
            assert_eq!(
                service.builder.runs[0].detail.as_deref(),
                Some("Project publication currently supports Windows only")
            );
            assert!(generated.staging.join("generation-manifest.json").is_file());
            assert!(!root.join("client").exists());
        }
        service.board.link(&card.id, &generated.job_id).unwrap();
        let restored = crate::board::Board::new(&admin);
        assert_eq!(restored.cards[0].jobs.len(), 3);
        assert_eq!(restored.cards[0].column, crate::board::Column::InProgress);
    }
}
