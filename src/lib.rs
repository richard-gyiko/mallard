pub mod core;
pub mod file_processor;
pub mod index;
pub mod parser;
pub mod rules;
pub mod schema;
pub mod store;
pub mod walk;

pub use crate::core::{
    Anchor, BuildRequest, BuildSummary, Counters, Edge, EdgeKind, FileId, FileRecord, FileStatus,
    FileTiming, Finding, Metadata, ParseError, ParsedFile, ProcessOutcome, Symbol, SymbolId,
    SymbolKind,
};
pub use crate::file_processor::FileProcessor;
pub use crate::index::build;
