use std::collections::BTreeMap;
use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

pub const INDEXER_VERSION: &str = env!("CARGO_PKG_VERSION");

pub type FileId = i64;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SymbolId(pub String);

impl SymbolId {
    pub fn compute(file_path: &str, qualified_name: &str, kind: SymbolKind, signature: &str) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(file_path.as_bytes());
        hasher.update(&[0]);
        hasher.update(qualified_name.as_bytes());
        hasher.update(&[0]);
        hasher.update(kind.as_str().as_bytes());
        hasher.update(&[0]);
        hasher.update(signature.as_bytes());
        SymbolId(short_hash(hasher.finalize()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub fn short_hash(hash: blake3::Hash) -> String {
    hash.to_hex().as_str()[..32].to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Function,
    Method,
    Struct,
    Enum,
    Trait,
    Module,
    Const,
    Static,
    Macro,
    TypeAlias,
    Field,
    Variant,
    Other,
}

impl SymbolKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SymbolKind::Function => "function",
            SymbolKind::Method => "method",
            SymbolKind::Struct => "struct",
            SymbolKind::Enum => "enum",
            SymbolKind::Trait => "trait",
            SymbolKind::Module => "module",
            SymbolKind::Const => "const",
            SymbolKind::Static => "static",
            SymbolKind::Macro => "macro",
            SymbolKind::TypeAlias => "type_alias",
            SymbolKind::Field => "field",
            SymbolKind::Variant => "variant",
            SymbolKind::Other => "other",
        }
    }
}

impl FromStr for SymbolKind {
    type Err = MallardError;
    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "function" => SymbolKind::Function,
            "method" => SymbolKind::Method,
            "struct" => SymbolKind::Struct,
            "enum" => SymbolKind::Enum,
            "trait" => SymbolKind::Trait,
            "module" => SymbolKind::Module,
            "const" => SymbolKind::Const,
            "static" => SymbolKind::Static,
            "macro" => SymbolKind::Macro,
            "type_alias" => SymbolKind::TypeAlias,
            "field" => SymbolKind::Field,
            "variant" => SymbolKind::Variant,
            "other" => SymbolKind::Other,
            _ => return Err(MallardError::MalformedRow(format!("unknown symbol kind {s:?}"))),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Calls,
    Imports,
    Contains,
    TestsFor,
    TestedBy,
}

/// Per [ADR-0010](../../docs/decisions/0010-edge-confidence-tier.md):
/// - `Extracted` — resolved within the file at parse time.
/// - `Inferred` — resolved by the post-build cross-file resolver.
/// - `Ambiguous` — multiple callable candidates; resolver refused to pick.
/// - `Unresolved` — no candidate anywhere (typically stdlib / external).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeConfidence {
    Extracted,
    Inferred,
    Ambiguous,
    Unresolved,
}

impl EdgeConfidence {
    pub fn as_str(self) -> &'static str {
        match self {
            EdgeConfidence::Extracted => "extracted",
            EdgeConfidence::Inferred => "inferred",
            EdgeConfidence::Ambiguous => "ambiguous",
            EdgeConfidence::Unresolved => "unresolved",
        }
    }
}

impl FromStr for EdgeConfidence {
    type Err = MallardError;
    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "extracted" => EdgeConfidence::Extracted,
            "inferred" => EdgeConfidence::Inferred,
            "ambiguous" => EdgeConfidence::Ambiguous,
            "unresolved" => EdgeConfidence::Unresolved,
            _ => {
                return Err(MallardError::MalformedRow(format!(
                    "unknown edge confidence {s:?}"
                )));
            }
        })
    }
}

impl EdgeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EdgeKind::Calls => "calls",
            EdgeKind::Imports => "imports",
            EdgeKind::Contains => "contains",
            EdgeKind::TestsFor => "tests_for",
            EdgeKind::TestedBy => "tested_by",
        }
    }

    pub fn all() -> &'static [EdgeKind] {
        &[
            EdgeKind::Calls,
            EdgeKind::Imports,
            EdgeKind::Contains,
            EdgeKind::TestsFor,
            EdgeKind::TestedBy,
        ]
    }
}

impl FromStr for EdgeKind {
    type Err = MallardError;
    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "calls" => EdgeKind::Calls,
            "imports" => EdgeKind::Imports,
            "contains" => EdgeKind::Contains,
            "tests_for" => EdgeKind::TestsFor,
            "tested_by" => EdgeKind::TestedBy,
            _ => return Err(MallardError::MalformedRow(format!("unknown edge kind {s:?}"))),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anchor {
    pub start_byte: u64,
    pub end_byte: u64,
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Symbol {
    pub id: SymbolId,
    pub file_id: FileId,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub signature: String,
    pub anchor: Anchor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub src: SymbolId,
    pub dst: Option<SymbolId>,
    pub dst_unresolved: Option<String>,
    pub kind: EdgeKind,
    pub confidence: EdgeConfidence,
    pub file_id: FileId,
    pub order_key: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub rule_id: String,
    pub file_id: FileId,
    pub start_line: u32,
    pub end_line: u32,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseError {
    pub file_id: FileId,
    pub message: String,
    pub line: u32,
    pub col: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    Indexed,
    Unparseable,
    SkippedSize,
    SkippedBinary,
    SkippedSymlink,
    SkippedExtension,
}

impl FileStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            FileStatus::Indexed => "indexed",
            FileStatus::Unparseable => "unparseable",
            FileStatus::SkippedSize => "skipped:size",
            FileStatus::SkippedBinary => "skipped:binary",
            FileStatus::SkippedSymlink => "skipped:symlink",
            FileStatus::SkippedExtension => "skipped:extension",
        }
    }
}

impl FromStr for FileStatus {
    type Err = MallardError;
    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "indexed" => FileStatus::Indexed,
            "unparseable" => FileStatus::Unparseable,
            "skipped:size" => FileStatus::SkippedSize,
            "skipped:binary" => FileStatus::SkippedBinary,
            "skipped:symlink" => FileStatus::SkippedSymlink,
            "skipped:extension" => FileStatus::SkippedExtension,
            _ => return Err(MallardError::MalformedRow(format!("unknown file status {s:?}"))),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRecord {
    pub id: FileId,
    pub path: String,
    pub language: Option<String>,
    pub size_bytes: u64,
    pub status: FileStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metadata {
    pub sha: String,
    pub indexer_version: String,
    pub rule_set_hash: Option<String>,
    pub built_at: String,
    pub language_allow_list: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ParsedFile {
    pub file_id: FileId,
    pub symbols: Vec<Symbol>,
    pub edges: Vec<Edge>,
    pub parse_errors: Vec<ParseError>,
    pub parse_ms: u64,
    pub query_ms: u64,
}

#[derive(Debug, Clone)]
pub struct ProcessOutcome {
    pub file_record: FileRecord,
    pub parsed: Option<ParsedFile>,
    pub findings: Vec<Finding>,
    pub timing: Option<FileTiming>,
}

impl Counters {
    pub fn record(&mut self, outcome: &ProcessOutcome) {
        match outcome.file_record.status {
            FileStatus::Indexed => {
                if let Some(parsed) = &outcome.parsed {
                    self.symbols += parsed.symbols.len() as u64;
                    self.parse_errors += parsed.parse_errors.len() as u64;
                    for edge in &parsed.edges {
                        *self
                            .edges_by_kind
                            .entry(edge.kind.as_str().to_string())
                            .or_insert(0) += 1;
                    }
                    if parsed.parse_errors.is_empty() {
                        self.files_indexed += 1;
                    }
                }
                self.findings += outcome.findings.len() as u64;
            }
            FileStatus::Unparseable => {
                self.parse_errors += 1;
            }
            other => {
                *self
                    .files_skipped_by_reason
                    .entry(other.as_str().to_string())
                    .or_insert(0) += 1;
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct BuildRequest {
    pub root: PathBuf,
    pub sha: String,
    pub rules_path: Option<PathBuf>,
    pub out_path: PathBuf,
    pub max_file_bytes: u64,
    pub language_allow_list: Vec<String>,
    pub slowest_files_n: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Counters {
    pub files_indexed: u64,
    pub symbols: u64,
    pub edges_by_kind: BTreeMap<String, u64>,
    pub findings: u64,
    pub files_skipped_by_reason: BTreeMap<String, u64>,
    pub parse_errors: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTiming {
    pub path: String,
    pub parse_ms: u64,
    pub query_ms: u64,
    pub rules_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildSummary {
    pub sha: String,
    pub indexer_version: String,
    pub rule_set_hash: Option<String>,
    pub out_path: PathBuf,
    pub elapsed_ms: u64,
    pub counters: Counters,
    pub slowest_files: Vec<FileTiming>,
}

#[derive(Debug, thiserror::Error)]
pub enum MallardError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("duckdb error: {0}")]
    DuckDb(#[from] duckdb::Error),
    #[error("tree-sitter language error: {0}")]
    TsLanguage(#[from] tree_sitter::LanguageError),
    #[error("tree-sitter query error: {0}")]
    TsQuery(#[from] tree_sitter::QueryError),
    #[error("yaml error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid path: {0}")]
    InvalidPath(String),
    #[error("index not found: {0}")]
    IndexNotFound(PathBuf),
    #[error("index format version mismatch: found {found}, expected {expected}")]
    VersionMismatch { found: u32, expected: u32 },
    #[error("malformed row: {0}")]
    MalformedRow(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, MallardError>;
