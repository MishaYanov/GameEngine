//! Read-only agent bundle and native tool boundary for editor guidance.

mod bundle;
pub mod knowledge;
pub mod session;
mod tools;

pub use bundle::{Bundle, BundleError, CorpusId};
pub use tools::{SearchArgs, ToolDescriptor, ToolError, ToolId, ToolScope, ToolService};
