use super::{
	EmbeddingCache, EmbeddingProvider, ExactAssetLookup, KnowledgeChunk, KnowledgeIndex,
	KnowledgeResults, ReferenceHit, SearchScope,
	lexical::{MAX_QUERY_CHARS, MAX_RESULTS, cite, exact_asset, lexical_rank},
};
use crate::CorpusId;
use std::collections::{BTreeMap, BTreeSet};

const MAX_VECTOR_CANDIDATES: usize = 128;
const FUSION_OFFSET: f64 = 60.0;

pub fn hybrid_search(
	index: &KnowledgeIndex,
	scope: &SearchScope,
	query: &str,
	limit: usize,
	provider: Option<&dyn EmbeddingProvider>,
	cache: &mut EmbeddingCache,
) -> Vec<ReferenceHit> {
	let query = query.trim();
	let limit = limit.min(MAX_RESULTS);
	if limit == 0 || query.is_empty() || query.chars().count() > MAX_QUERY_CHARS {
		return Vec::new();
	}
	let lexical = lexical_rank(index, scope, query);
	let semantic =
		provider.and_then(|provider| semantic_rank(index, scope, query, &lexical, provider, cache));
	let Some(semantic) = semantic.filter(|rank| !rank.is_empty()) else {
		return lexical.into_iter().take(limit).map(cite).collect();
	};
	fuse(&lexical, &semantic)
		.into_iter()
		.take(limit)
		.map(cite)
		.collect()
}

pub fn hybrid_search_with_exact_asset(
	index: &KnowledgeIndex,
	scope: &SearchScope,
	query: &str,
	limit: usize,
	assets: &dyn ExactAssetLookup,
	provider: Option<&dyn EmbeddingProvider>,
	cache: &mut EmbeddingCache,
) -> KnowledgeResults {
	KnowledgeResults {
		exact_asset: exact_asset(scope, query, assets),
		reference_hits: hybrid_search(index, scope, query, limit, provider, cache),
	}
}

fn semantic_rank<'a>(
	index: &'a KnowledgeIndex,
	scope: &SearchScope,
	query: &str,
	lexical: &[&'a KnowledgeChunk],
	provider: &dyn EmbeddingProvider,
	cache: &mut EmbeddingCache,
) -> Option<Vec<&'a KnowledgeChunk>> {
	// Give top lexical candidates priority when the corpus exceeds the vector budget.
	let mut seen = BTreeSet::new();
	let candidates: Vec<_> = lexical
		.iter()
		.copied()
		.chain(index.chunks(scope))
		.filter(|chunk| seen.insert(chunk_key(chunk)))
		.take(MAX_VECTOR_CANDIDATES)
		.collect();
	if candidates.is_empty() {
		return Some(Vec::new());
	}
	let query = cache.query_vector(provider, query)?;
	let mut ranked = Vec::new();
	for chunk in candidates {
		let vector = cache.chunk_vector(provider, chunk)?;
		let similarity: f64 = query
			.iter()
			.zip(vector)
			.map(|(&a, &b)| f64::from(a) * f64::from(b))
			.sum();
		if similarity > 0.0 {
			ranked.push((similarity, chunk));
		}
	}
	ranked.sort_by(|left, right| right.0.total_cmp(&left.0));
	Some(ranked.into_iter().map(|(_, chunk)| chunk).collect())
}

fn fuse<'a>(
	lexical: &[&'a KnowledgeChunk],
	vectors: &[&'a KnowledgeChunk],
) -> Vec<&'a KnowledgeChunk> {
	let mut by_chunk = BTreeMap::new();
	for ranking in [lexical, vectors] {
		for (rank, &chunk) in ranking.iter().take(MAX_VECTOR_CANDIDATES).enumerate() {
			let score = 1.0 / (FUSION_OFFSET + (rank + 1) as f64);
			let (_, fused_score) = by_chunk.entry(chunk_key(chunk)).or_insert((chunk, 0.0));
			*fused_score += score;
		}
	}
	let mut combined: Vec<_> = by_chunk.into_values().collect();
	combined.sort_by(|left, right| right.1.total_cmp(&left.1));
	combined.into_iter().map(|(chunk, _)| chunk).collect()
}

fn chunk_key(chunk: &KnowledgeChunk) -> (u8, &str, usize) {
	let corpus = match chunk.citation.corpus {
		CorpusId::EngineHelp => 0,
		CorpusId::ActiveProject => 1,
	};
	(corpus, &chunk.citation.source_id, chunk.citation.ordinal)
}
