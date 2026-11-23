//! Bounded retrieval over explicit engine and active-project sources.

mod corpus;
mod index;
mod lexical;

pub use corpus::{ApprovedSource, Compatibility, CorpusError, CorpusManifest, SearchScope};
pub use index::{Citation, ContentHash, KnowledgeChunk, KnowledgeIndex, RefreshStats};
pub use lexical::{
	ExactAsset, ExactAssetLookup, KnowledgeResults, ReferenceHit, lexical_search,
	search_with_exact_asset,
};
