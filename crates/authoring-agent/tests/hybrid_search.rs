use authoring_agent::{
	CorpusId,
	knowledge::{
		ApprovedSource, Compatibility, CorpusManifest, EmbeddingCache, EmbeddingModel,
		EmbeddingProvider, ExactAsset, ExactAssetLookup, KnowledgeIndex, hybrid_search,
		hybrid_search_with_exact_asset, lexical_search,
	},
};
use std::{cell::Cell, fs, path::PathBuf};

struct Scratch(PathBuf);
impl Scratch {
	fn new() -> Self {
		let path = std::env::temp_dir()
			.canonicalize()
			.unwrap()
			.join(format!("agent-hybrid-{}", std::process::id()));
		fs::create_dir(&path).unwrap();
		Self(path)
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

struct FakeModel {
	revision: &'static str,
	dimensions: usize,
	corpus_calls: Cell<usize>,
	offline: bool,
}

impl FakeModel {
	fn new(revision: &'static str, dimensions: usize, offline: bool) -> Self {
		Self {
			revision,
			dimensions,
			corpus_calls: Cell::new(0),
			offline,
		}
	}
}

impl EmbeddingProvider for FakeModel {
	fn model(&self) -> EmbeddingModel {
		EmbeddingModel::new("mock-cpu", self.revision, self.dimensions)
	}

	fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
		assert!(
			!text.contains("Foreign"),
			"a different project's text left scope"
		);
		if self.offline {
			return Err("offline".into());
		}
		if text.contains("gravity") || text.contains("camera") || text.contains("assets") {
			self.corpus_calls.set(self.corpus_calls.get() + 1);
		}
		if text.contains("gravity") || text.contains("fall downward") || text == "cube-asset" {
			Ok(vec![1.0, 0.0])
		} else {
			Ok(vec![0.0, 1.0])
		}
	}
}

struct Assets;
impl ExactAssetLookup for Assets {
	fn find_exact(&self, project_id: &str, id: &str) -> Option<ExactAsset> {
		(project_id == "workshop" && id == "cube-asset").then(|| ExactAsset {
			project_id: project_id.into(),
			id: id.into(),
			label: "Cube".into(),
			relative_path: "models/cube.glb".into(),
		})
	}
}

#[test]
fn hybrid_improves_paraphrases_uses_scoped_cache_and_falls_back_safely() {
	let scratch = Scratch::new();
	let engine = scratch.0.join("engine");
	let local = scratch.0.join("project");
	let foreign = scratch.0.join("foreign");
	for path in [&engine, &local, &foreign] {
		fs::create_dir(path).unwrap();
	}
	fs::write(
		engine.join("physics.md"),
		"# Physics\nEnable gravity using the physics settings.\n",
	)
	.unwrap();
	fs::write(
		engine.join("camera.md"),
		"# Camera\nSelect an active camera for play.\n",
	)
	.unwrap();
	fs::write(local.join("assets.json"), "Workshop assets catalog.").unwrap();
	fs::write(
		foreign.join("assets.json"),
		"Foreign project's assets and secrets.",
	)
	.unwrap();
	let manifest = |id, path| {
		CorpusManifest::new(
			Compatibility::new("0.1.0", 1),
			&engine,
			Some((id, path)),
			vec![
				ApprovedSource::new(CorpusId::EngineHelp, "physics", "physics.md").unwrap(),
				ApprovedSource::new(CorpusId::EngineHelp, "camera", "camera.md").unwrap(),
				ApprovedSource::new(CorpusId::ActiveProject, "props", "assets.json").unwrap(),
			],
		)
		.unwrap()
	};
	let local = manifest("workshop", &local);
	let foreign = manifest("foreign", &foreign);
	let mut index = KnowledgeIndex::default();
	index.refresh(&local).unwrap();
	index.refresh(&foreign).unwrap();
	let scope = local.scope();
	let mut cache = EmbeddingCache::default();

	let paraphrase = "Why would actors fall downward?";
	assert!(lexical_search(&index, scope, paraphrase, 1).is_empty());
	let first_model = FakeModel::new("one", 2, false);
	let semantic = hybrid_search(&index, scope, paraphrase, 1, Some(&first_model), &mut cache);
	assert_eq!(semantic[0].citation.source_id, "physics");
	assert_eq!(first_model.corpus_calls.get(), 3);
	assert_eq!(cache.cached_count(), 3);
	assert_eq!(
		hybrid_search(&index, scope, paraphrase, 1, Some(&first_model), &mut cache),
		semantic
	);
	assert_eq!(first_model.corpus_calls.get(), 3);

	// Changed bytes miss the old digest. Unchanged files retain their cached vectors.
	fs::write(
		engine.join("physics.md"),
		"# Physics\nEnable gravity on the active body.\n",
	)
	.unwrap();
	index.refresh(&local).unwrap();
	assert_eq!(
		hybrid_search(&index, scope, paraphrase, 1, Some(&first_model), &mut cache)[0]
			.citation
			.source_id,
		"physics"
	);
	assert_eq!(first_model.corpus_calls.get(), 4);

	let second_model = FakeModel::new("two", 2, false);
	let matched = hybrid_search_with_exact_asset(
		&index,
		scope,
		"cube-asset",
		2,
		&Assets,
		Some(&second_model),
		&mut cache,
	);
	assert_eq!(
		matched.exact_asset.unwrap().relative_path,
		"models/cube.glb"
	);
	assert_eq!(second_model.corpus_calls.get(), 3);
	assert_eq!(cache.cached_count(), 3);

	let known = lexical_search(&index, scope, "enable gravity", 4);
	let offline = FakeModel::new("offline", 2, true);
	assert_eq!(
		hybrid_search(
			&index,
			scope,
			"enable gravity",
			4,
			Some(&offline),
			&mut cache
		),
		known
	);
	let invalid = FakeModel::new("two", 3, false);
	assert_eq!(
		hybrid_search(
			&index,
			scope,
			"enable gravity",
			4,
			Some(&invalid),
			&mut cache
		),
		known
	);
	assert_eq!(
		hybrid_search(&index, scope, "enable gravity", 4, None, &mut cache),
		known
	);
}
