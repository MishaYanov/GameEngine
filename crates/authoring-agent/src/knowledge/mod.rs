//! Bounded retrieval over explicit engine and active-project sources.

mod corpus;
mod index;

pub use corpus::{ApprovedSource, Compatibility, CorpusError, CorpusManifest, SearchScope};
pub use index::{Citation, ContentHash, KnowledgeChunk, KnowledgeIndex, RefreshStats};
