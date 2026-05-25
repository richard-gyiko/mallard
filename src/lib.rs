pub mod core;
pub mod extractor;
pub mod extractor_common;
pub mod extractor_python;
pub mod extractor_typescript;
pub mod file_processor;
pub mod index;
pub mod parse_errors;
pub mod parsed_source;
pub mod pr_review;
pub mod query;
pub mod rules;
pub mod schema;
pub mod store;
pub mod walk;

pub use crate::core::{
    Anchor, BuildRequest, BuildSummary, Counters, Edge, EdgeConfidence, EdgeKind, FileId,
    FileRecord, FileStatus, FileTiming, Finding, MallardError, Metadata, ParseError, ParsedFile,
    ProcessOutcome, Result, Symbol, SymbolId, SymbolKind,
};
pub use crate::file_processor::FileProcessor;
pub use crate::index::build;
pub use crate::query::{
    BlastRadius, Direction, FileEdgeBundle, FileRecordOut, FindingFilter, FindingRecord,
    IndexReader, MetadataRecord, ModifiedSymbol, NeighborEdge, QueryRequest, QueryResult,
    Subgraph, SymbolDiff, SymbolRecord, UnresolvedCallerHit, symbol_diff,
};
