use authoring_agent::{
	CorpusId,
	knowledge::{
		ApprovedSource, Compatibility, CorpusManifest, ExactAsset, ExactAssetLookup,
		KnowledgeIndex, SearchScope, lexical_search, search_with_exact_asset,
	},
};
use std::{fs, path::PathBuf};
const CUBE: &str = "95000000-0000-4000-8000-000000000001";

struct Scratch(PathBuf);
impl Scratch {
	fn new() -> Self {
		let path = std::env::temp_dir()
			.canonicalize()
			.unwrap()
			.join(format!("agent-lexical-{}", std::process::id()));
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

struct Assets;
impl ExactAssetLookup for Assets {
	fn find_exact(&self, project_id: &str, id: &str) -> Option<ExactAsset> {
		(project_id == "alpha" && id == CUBE).then(|| ExactAsset {
			project_id: project_id.into(),
			id: id.into(),
			label: "Workshop Cube".into(),
			relative_path: "models/cube.glb".into(),
		})
	}
}

#[test]
fn exact_ids_and_ranked_current_citations_stay_distinct() {
	let root = Scratch::new();
	let engine = root.0.join("engine");
	let alpha = root.0.join("alpha");
	let beta = root.0.join("beta");
	for path in [&engine, &alpha, &beta] {
		fs::create_dir(path).unwrap();
	}
	fs::write(
		engine.join("physics.md"),
		"# Enable Gravity\nAdd a Box Collider and enable gravity in Physics.\n",
	)
	.unwrap();
	fs::write(
		engine.join("overview.md"),
		"# Editor Overview\nThe workspace includes gravity configuration.\n",
	)
	.unwrap();
	fs::write(
		alpha.join("props.json"),
		format!("Workshop Cube catalog identifier {CUBE}."),
	)
	.unwrap();
	fs::write(
		beta.join("props.json"),
		"Foreign project enables gravity on Ocean props.",
	)
	.unwrap();
	let manifest = |version, id, path| {
		CorpusManifest::new(
			Compatibility::new(version, 1),
			&engine,
			Some((id, path)),
			vec![
				ApprovedSource::new(CorpusId::EngineHelp, "physics", "physics.md").unwrap(),
				ApprovedSource::new(CorpusId::EngineHelp, "overview", "overview.md").unwrap(),
				ApprovedSource::new(CorpusId::ActiveProject, "props", "props.json").unwrap(),
			],
		)
		.unwrap()
	};
	let mut index = KnowledgeIndex::default();
	// Pin a separate engine version before refreshing the current sources.
	let outdated = manifest("0.0.9", "alpha", &alpha);
	index.refresh(&outdated).unwrap();
	let current = manifest("0.1.0", "alpha", &alpha);
	let foreign = manifest("0.1.0", "beta", &beta);
	index.refresh(&current).unwrap();
	index.refresh(&foreign).unwrap();

	let results = search_with_exact_asset(&index, current.scope(), CUBE, 8, &Assets);
	assert_eq!(results.exact_asset.unwrap().label, "Workshop Cube");
	assert_eq!(results.reference_hits[0].citation.source_id, "props");
	assert!(
		search_with_exact_asset(&index, current.scope(), "cube-unknown", 4, &Assets)
			.exact_asset
			.is_none()
	);
	assert!(
		search_with_exact_asset(&index, foreign.scope(), CUBE, 4, &Assets)
			.exact_asset
			.is_none()
	);

	let query = "how to enable gravity";
	let hits = lexical_search(&index, current.scope(), query, 8);
	assert_eq!(hits[0].citation.source_id, "physics");
	assert_eq!(hits[0].citation.heading, "Enable Gravity");
	assert!(
		hits.iter()
			.all(|hit| hit.citation.compatibility.engine_version == "0.1.0")
	);
	assert!(!hits.iter().any(|hit| hit.excerpt.contains("Ocean")));
	let revision = hits[0].citation.source_revision;
	assert_eq!(
		lexical_search(&index, current.scope(), "enable grav", 1)[0]
			.citation
			.source_id,
		"physics"
	);
	assert_eq!(lexical_search(&index, current.scope(), query, 8), hits);

	fs::write(
		engine.join("physics.md"),
		"# Box Colliders\nSet a supported collider shape.\n",
	)
	.unwrap();
	index.refresh(&current).unwrap();
	assert!(
		lexical_search(&index, current.scope(), "enable gravity", 8)
			.iter()
			.all(|hit| hit.citation.source_id != "physics"
				&& hit.citation.source_revision != revision)
	);
	assert!(!lexical_search(&index, outdated.scope(), "enable gravity", 8).is_empty());
	let other_schema = SearchScope::new(Compatibility::new("0.1.0", 2), Some("alpha".into()));
	assert!(lexical_search(&index, &other_schema, "gravity", 8).is_empty());
}
