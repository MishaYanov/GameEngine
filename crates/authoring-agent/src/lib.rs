//! Read-only agent bundle and native tool boundary for editor guidance.

mod bundle;
mod tools;

pub use bundle::{Bundle, BundleError, CorpusId};
pub use tools::{SearchArgs, ToolDescriptor, ToolError, ToolId, ToolScope, ToolService};
