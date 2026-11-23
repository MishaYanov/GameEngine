//! Bounded retrieval over explicit engine and active-project sources.

mod corpus;
mod embeddings;
mod hybrid;
mod index;
mod lexical;

pub use corpus::{ApprovedSource, Compatibility, CorpusError, CorpusManifest, SearchScope};
pub use embeddings::{EmbeddingCache, EmbeddingModel, EmbeddingProvider};
pub use hybrid::{hybrid_search, hybrid_search_with_exact_asset};
pub use index::{Citation, ContentHash, KnowledgeChunk, KnowledgeIndex, RefreshStats};
pub use lexical::{
	ExactAsset, ExactAssetLookup, KnowledgeResults, ReferenceHit, lexical_search,
	search_with_exact_asset,
};
