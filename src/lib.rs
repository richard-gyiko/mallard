pub mod core;
pub mod extractor;
pub mod file_processor;
pub mod index;
pub mod parsed_source;
pub mod query;
pub mod rules;
pub mod schema;
pub mod store;
pub mod walk;

pub use crate::core::{
    Anchor, BuildRequest, BuildSummary, Counters, Edge, EdgeKind, FileId, FileRecord, FileStatus,
    FileTiming, Finding, MallardError, Metadata, ParseError, ParsedFile, ProcessOutcome, Result,
    Symbol, SymbolId, SymbolKind,
};
pub use crate::file_processor::FileProcessor;
pub use crate::index::build;
pub use crate::query::{
    Direction, FileRecordOut, FindingFilter, FindingRecord, IndexReader, MetadataRecord,
    NeighborEdge, QueryRequest, QueryResult, Subgraph, SymbolRecord,
};
