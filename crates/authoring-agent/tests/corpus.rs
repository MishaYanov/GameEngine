use authoring_agent::{
	CorpusId,
	knowledge::{
		ApprovedSource, Compatibility, CorpusError, CorpusManifest, KnowledgeIndex, RefreshStats,
		SearchScope,
	},
};
use std::{
	fs,
	path::{Path, PathBuf},
	sync::atomic::{AtomicU32, Ordering},
};

struct Scratch(PathBuf);

impl Scratch {
	fn new() -> Self {
		static NEXT: AtomicU32 = AtomicU32::new(0);
		let directory = std::env::temp_dir().canonicalize().unwrap().join(format!(
			"agent-corpus-{}-{}",
			std::process::id(),
			NEXT.fetch_add(1, Ordering::Relaxed)
		));
		fs::create_dir(&directory).unwrap();
		Self(directory)
	}

	fn child(&self, name: &str) -> PathBuf {
		let path = self.0.join(name);
		fs::create_dir(&path).unwrap();
		path
	}
}

impl Drop for Scratch {
	fn drop(&mut self) {
		if self
			.0
			.canonicalize()
			.is_ok_and(|resolved| resolved == self.0)
		{
			let _ = fs::remove_dir_all(&self.0);
		}
	}
}

fn manifest(engine: &Path, project: (&str, &Path)) -> CorpusManifest {
	CorpusManifest::new(
		Compatibility::new("0.1.0", 1),
		engine,
		Some(project),
		vec![
			ApprovedSource::new(CorpusId::EngineHelp, "editor-help", "help.md").unwrap(),
			ApprovedSource::new(CorpusId::ActiveProject, "asset-metadata", "assets.json").unwrap(),
		],
	)
	.unwrap()
}

#[test]
fn refreshes_scoped_sources_and_preserves_deterministic_revisions() {
	let directory = Scratch::new();
	let engine = directory.child("engine");
	let alpha = directory.child("alpha");
	let beta = directory.child("beta");
	fs::write(
		engine.join("help.md"),
		"# Physics\nEnable gravity.\n\n## Camera\nSelect the game camera.\n",
	)
	.unwrap();
	fs::write(alpha.join("assets.json"), "Forest prop metadata").unwrap();
	fs::write(alpha.join("ignored.txt"), "not approved").unwrap();
	fs::write(beta.join("assets.json"), "Ocean prop metadata").unwrap();
	let alpha_manifest = manifest(&engine, ("alpha", &alpha));
	let beta_manifest = manifest(&engine, ("beta", &beta));
	let mut index = KnowledgeIndex::default();
	assert_eq!(
		index.refresh(&alpha_manifest).unwrap(),
		RefreshStats {
			added: 2,
			..Default::default()
		}
	);
	let stable: Vec<_> = index.chunks(alpha_manifest.scope()).cloned().collect();
	assert_eq!(stable.len(), 3);
	assert_eq!(stable[0].citation.heading, "Physics");
	assert_eq!(stable[1].citation.heading, "Camera");
	assert_eq!(stable[2].citation.project_id.as_deref(), Some("alpha"));
	assert_eq!(
		stable[0].citation.source_revision,
		stable[1].citation.source_revision
	);
	assert_ne!(
		stable[0].citation.content_hash,
		stable[1].citation.content_hash
	);
	assert_eq!(
		index.refresh(&alpha_manifest).unwrap(),
		RefreshStats::default()
	);
	assert_eq!(
		index
			.chunks(alpha_manifest.scope())
			.cloned()
			.collect::<Vec<_>>(),
		stable
	);

	assert_eq!(
		index.refresh(&beta_manifest).unwrap(),
		RefreshStats {
			added: 1,
			..Default::default()
		}
	);
	assert!(
		index
			.chunks(beta_manifest.scope())
			.any(|chunk| chunk.text == "Ocean prop metadata")
	);
	assert!(
		!index
			.chunks(beta_manifest.scope())
			.any(|chunk| chunk.text.starts_with("Forest"))
	);
	assert!(
		index
			.chunks(alpha_manifest.scope())
			.any(|chunk| chunk.text == "Forest prop metadata")
	);
	assert!(
		!index
			.chunks(alpha_manifest.scope())
			.any(|chunk| chunk.text.contains("not approved"))
	);
	let incompatible = SearchScope::new(Compatibility::new("0.1.0", 2), Some("alpha".into()));
	assert_eq!(index.chunks(&incompatible).count(), 0);

	fs::write(alpha.join("assets.json"), "Changed forest metadata").unwrap();
	assert_eq!(
		index.refresh(&alpha_manifest).unwrap(),
		RefreshStats {
			updated: 1,
			..Default::default()
		}
	);
	let mut snapshot = Vec::new();
	index
		.write_snapshot(alpha_manifest.scope(), &mut snapshot)
		.unwrap();
	let parsed: serde_json::Value = serde_json::from_slice(&snapshot).unwrap();
	assert_eq!(parsed["version"], 1);
	assert_eq!(parsed["project_id"], "alpha");
	assert!(
		parsed["chunks"]
			.as_array()
			.unwrap()
			.iter()
			.any(|entry| entry["text"] == "Changed forest metadata")
	);
	assert!(!String::from_utf8(snapshot).unwrap().contains("Ocean"));
	fs::remove_file(alpha.join("assets.json")).unwrap();
	assert_eq!(
		index.refresh(&alpha_manifest).unwrap(),
		RefreshStats {
			removed: 1,
			..Default::default()
		}
	);
	assert_eq!(index.chunks(alpha_manifest.scope()).count(), 2);
	assert_eq!(index.chunks(beta_manifest.scope()).count(), 3);
}

#[test]
fn rejects_sensitive_paths_and_fails_refresh_without_partial_replacement() {
	for path in [
		"../foreign/help.md",
		".env.json",
		".git/config.txt",
		".ssh/credentials.json",
		"target/debug/generated.json",
		"nested/credentials.json",
		"node_modules/secret.json",
		"assets/private-key.txt",
		"not-text.wav",
	] {
		assert!(
			matches!(
				ApprovedSource::new(CorpusId::ActiveProject, "malicious", path),
				Err(CorpusError::UnapprovedPath(_))
			),
			"{path}"
		);
	}
	let directory = Scratch::new();
	let engine = directory.child("engine");
	let alpha = directory.child("alpha");
	let source = engine.join("help.md");
	assert!(matches!(
		ApprovedSource::new(CorpusId::EngineHelp, "absolute", source.clone()),
		Err(CorpusError::UnapprovedPath(_))
	));
	let manifest = manifest(&engine, ("alpha", &alpha));
	fs::write(&source, format!("# Heading\n{}\n", "é".repeat(1500))).unwrap();
	fs::write(alpha.join("assets.json"), "Approved metadata").unwrap();
	let mut index = KnowledgeIndex::default();
	index.refresh(&manifest).unwrap();
	let previous: Vec<_> = index.chunks(manifest.scope()).cloned().collect();
	assert_eq!(previous.len(), 3);
	assert_eq!(previous[0].citation.heading, "Heading");
	assert!(previous.iter().all(|chunk| chunk.text.len() <= 1800));

	fs::write(&source, "Updated engine text").unwrap();
	fs::write(alpha.join("assets.json"), "a".repeat(65_537)).unwrap();
	assert!(matches!(
		index.refresh(&manifest),
		Err(CorpusError::Budget("source bytes"))
	));
	assert_eq!(
		index.chunks(manifest.scope()).cloned().collect::<Vec<_>>(),
		previous
	);
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_approved_files_that_escape_the_corpus() {
	let directory = Scratch::new();
	let engine = directory.child("engine");
	let alpha = directory.child("alpha");
	fs::write(directory.0.join("outside.txt"), "Outside both roots").unwrap();
	std::os::unix::fs::symlink(directory.0.join("outside.txt"), engine.join("help.md")).unwrap();
	let manifest = manifest(&engine, ("alpha", &alpha));
	assert!(matches!(
		KnowledgeIndex::default().refresh(&manifest),
		Err(CorpusError::UnapprovedPath(_))
	));
}
