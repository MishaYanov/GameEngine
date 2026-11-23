use super::{Citation, KnowledgeChunk, KnowledgeIndex, SearchScope};
use serde::Serialize;
use std::collections::BTreeSet;

pub(super) const MAX_QUERY_CHARS: usize = 512;
const MAX_QUERY_TERMS: usize = 16;
pub(super) const MAX_RESULTS: usize = 16;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReferenceHit {
	pub citation: Citation,
	pub excerpt: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ExactAsset {
	pub project_id: String,
	pub id: String,
	pub label: String,
	pub relative_path: String,
}

/// Current authored assets come from the live scoped adapter, not stored help text.
pub trait ExactAssetLookup {
	fn find_exact(&self, project_id: &str, id: &str) -> Option<ExactAsset>;
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct KnowledgeResults {
	pub exact_asset: Option<ExactAsset>,
	pub reference_hits: Vec<ReferenceHit>,
}

pub fn lexical_search(
	index: &KnowledgeIndex,
	scope: &SearchScope,
	query: &str,
	limit: usize,
) -> Vec<ReferenceHit> {
	lexical_rank(index, scope, query)
		.into_iter()
		.take(limit.min(MAX_RESULTS))
		.map(cite)
		.collect()
}

pub fn search_with_exact_asset(
	index: &KnowledgeIndex,
	scope: &SearchScope,
	query: &str,
	limit: usize,
	assets: &dyn ExactAssetLookup,
) -> KnowledgeResults {
	KnowledgeResults {
		exact_asset: exact_asset(scope, query, assets),
		reference_hits: lexical_search(index, scope, query, limit),
	}
}

pub(super) fn exact_asset(
	scope: &SearchScope,
	query: &str,
	assets: &dyn ExactAssetLookup,
) -> Option<ExactAsset> {
	let project_id = scope.project_id.as_deref()?;
	let id = query.trim();
	if id.is_empty()
		|| id.len() > 128
		|| !id
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
	{
		return None;
	}
	assets
		.find_exact(project_id, id)
		.filter(|record| record.id == id && record.project_id == project_id)
}

pub(super) fn cite(chunk: &KnowledgeChunk) -> ReferenceHit {
	ReferenceHit {
		citation: chunk.citation.clone(),
		excerpt: chunk.text.clone(),
	}
}

pub(super) fn lexical_rank<'a>(
	index: &'a KnowledgeIndex,
	scope: &SearchScope,
	query: &str,
) -> Vec<&'a KnowledgeChunk> {
	let query = query.trim();
	if query.is_empty() || query.chars().count() > MAX_QUERY_CHARS {
		return Vec::new();
	}
	let normalized = query.to_lowercase();
	let terms: BTreeSet<_> = words(&normalized).take(MAX_QUERY_TERMS).collect();
	if terms.is_empty() {
		return Vec::new();
	}

	let mut ranked: Vec<_> = index
		.chunks(scope)
		.filter_map(|chunk| {
			let name = chunk.citation.source_id.to_lowercase();
			let heading = chunk.citation.heading.to_lowercase();
			let text = chunk.text.to_lowercase();
			let weights = 7 * matching_terms(&heading, &terms)
				+ 4 * matching_terms(&name, &terms)
				+ matching_terms(&text, &terms);
			let exact = 2 * usize::from(heading.contains(&normalized))
				+ usize::from(text.contains(&normalized));
			let score = weights + exact;
			(score > 0).then_some((score, chunk))
		})
		.collect();
	// Preserve the index's deterministic corpus, source ID and ordinal order on a tie.
	ranked.sort_by_key(|row| std::cmp::Reverse(row.0));
	ranked.into_iter().map(|(_, chunk)| chunk).collect()
}

fn words(text: &str) -> impl Iterator<Item = &str> {
	text.split(|character: char| !character.is_alphanumeric())
		.filter(|term| !term.is_empty())
}

fn matching_terms(text: &str, terms: &BTreeSet<&str>) -> usize {
	let words: BTreeSet<_> = words(text).collect();
	terms
		.iter()
		.map(|term| {
			if words.contains(term) {
				2
			} else if term.len() >= 3
				&& words
					.range(*term..)
					.next()
					.is_some_and(|word| word.starts_with(*term))
			{
				1
			} else {
				0
			}
		})
		.sum()
}
