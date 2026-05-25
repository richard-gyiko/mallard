use std::path::Path;
use std::str::FromStr;

use duckdb::{Connection, OptionalExt, params};
use serde::{Deserialize, Serialize};

use crate::core::{
    Anchor, EdgeConfidence, EdgeKind, FileId, FileStatus, MallardError, Result, SymbolId,
    SymbolKind,
};
use crate::schema::{self, cols, metadata_keys, tables};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Out,
    In,
    Both,
}

impl FromStr for Direction {
    type Err = MallardError;
    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "out" => Direction::Out,
            "in" => Direction::In,
            "both" => Direction::Both,
            other => return Err(MallardError::Other(format!("unknown direction {other:?}"))),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolRecord {
    pub id: SymbolId,
    pub file_id: FileId,
    pub path: String,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub signature: String,
    pub anchor: Anchor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NeighborEdge {
    pub kind: EdgeKind,
    pub confidence: EdgeConfidence,
    pub direction: Direction,
    pub src: SymbolRecord,
    pub dst: Option<SymbolRecord>,
    pub dst_unresolved: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subgraph {
    pub nodes: Vec<SymbolRecord>,
    pub edges: Vec<NeighborEdge>,
    pub max_depth_reached: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindingRecord {
    pub rule_id: String,
    pub file_id: FileId,
    pub path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindingFilter {
    pub rule_id: Option<String>,
    pub path_prefix: Option<String>,
    pub symbol_id: Option<SymbolId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRecordOut {
    pub file_id: FileId,
    pub path: String,
    pub language: Option<String>,
    pub size_bytes: u64,
    pub status: FileStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetadataRecord {
    pub sha: Option<String>,
    pub indexer_version: Option<String>,
    pub rule_set_hash: Option<String>,
    pub built_at: Option<String>,
    pub language_allow_list: Vec<String>,
    pub index_format_version: u32,
}

/// Lightweight cross-index symbol diff. Compares two `IndexReader`
/// snapshots by `(qualified_name, path)` keys and partitions symbols into
/// added (HEAD-only), removed (BASE-only), and modified (present in both
/// but with a different anchor byte range — typically a body edit).
/// Cheaper than `pr_review::run`: no rule findings, no diff-hunk overlap,
/// no comment budget — just the structural delta.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolDiff {
    pub added: Vec<SymbolRecord>,
    pub removed: Vec<SymbolRecord>,
    pub modified: Vec<ModifiedSymbol>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModifiedSymbol {
    pub qualified_name: String,
    pub path: String,
    pub base: SymbolRecord,
    pub head: SymbolRecord,
}

/// Diff two opened indexes. Symbols match by `(qualified_name, path)`; a
/// pair counts as `modified` when the anchor byte range or the line span
/// differs. `path` change without rename is treated as remove+add — that
/// matches what reviewers see in `git diff` (file relocation surfaces as
/// add + delete unless `--find-renames` is set, which mallard doesn't try
/// to second-guess).
pub fn symbol_diff(base: &IndexReader, head: &IndexReader) -> Result<SymbolDiff> {
    let base_syms = all_symbols(base)?;
    let head_syms = all_symbols(head)?;

    use std::collections::HashMap;
    // Key includes signature so overloaded methods (`Outer::tag(&self)` vs
    // `Outer::tag(&self, n: u32)`) don't false-match. Multiple symbols can
    // still collapse onto one key when signatures are absent (e.g.
    // attributes / consts) — for those we group into a Vec and pop
    // one-to-one. Stable order from `all_symbols` keeps matching
    // deterministic across runs.
    type Key = (String, String, String);
    let mut base_map: HashMap<Key, Vec<SymbolRecord>> = HashMap::with_capacity(base_syms.len());
    for s in base_syms {
        let key = (s.qualified_name.clone(), s.path.clone(), s.signature.clone());
        base_map.entry(key).or_default().push(s);
    }

    let mut added = Vec::new();
    let mut modified = Vec::new();

    for h in head_syms {
        let key = (h.qualified_name.clone(), h.path.clone(), h.signature.clone());
        match base_map.get_mut(&key) {
            None => added.push(h),
            Some(bucket) if bucket.is_empty() => added.push(h),
            Some(bucket) => {
                let b = bucket.remove(0);
                if b.anchor != h.anchor {
                    modified.push(ModifiedSymbol {
                        qualified_name: h.qualified_name.clone(),
                        path: h.path.clone(),
                        base: b,
                        head: h,
                    });
                }
            }
        }
    }

    let removed: Vec<SymbolRecord> = base_map.into_values().flatten().collect();

    Ok(SymbolDiff {
        added,
        removed,
        modified,
    })
}

fn all_symbols(reader: &IndexReader) -> Result<Vec<SymbolRecord>> {
    reader.all_symbols()
}

/// Composite blast-radius for a qualified-name lookup. Agent-friendly shape:
/// the symbol itself, its inbound callers, outbound callees, and the subset of
/// callers that look like test seams. `other_qname_matches` surfaces ambiguity
/// when the qname matched more than one symbol (e.g. shared short names across
/// modules) — agents disambiguate via `path` + `kind` on the matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlastRadius {
    pub symbol: SymbolRecord,
    pub callers: Vec<SymbolRecord>,
    pub callees: Vec<SymbolRecord>,
    pub test_seams: Vec<SymbolRecord>,
    pub other_qname_matches: Vec<SymbolRecord>,
}

/// Bulk per-file output: every symbol defined in the file, plus its
/// outbound and inbound edges (peer-enriched). Symbols with zero edges
/// still appear with empty `outbound` / `inbound` so callers can
/// `comm`-diff bundles across base/head without re-querying.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEdgeBundle {
    pub symbol: SymbolRecord,
    pub outbound: Vec<NeighborEdge>,
    pub inbound: Vec<NeighborEdge>,
}

/// One unresolved-edge match for the deletion-sanity scan. Surfaces a call
/// site in the index that references a name that didn't resolve to any
/// symbol — useful for spotting orphan callers of symbols removed in a PR.
/// `confidence` distinguishes truly-unresolved (typically stdlib/external)
/// from ambiguous-name cases the resolver refused to pick.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnresolvedCallerHit {
    pub unresolved_name: String,
    pub edge_kind: EdgeKind,
    pub confidence: EdgeConfidence,
    pub caller: SymbolRecord,
}

/// Adapter-facing request that crosses the query seam. CLI marshals argv into
/// one of these; future adapters (MCP, HTTP) build the same shape. The typed
/// per-method API stays public for in-process Rust callers (retrieval,
/// PR review) — see CONTEXT.md.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QueryRequest {
    LookupSymbol { id: SymbolId },
    Neighbors {
        id: SymbolId,
        #[serde(default)]
        kinds: Vec<EdgeKind>,
        direction: Direction,
    },
    Expand {
        id: SymbolId,
        depth: u32,
        #[serde(default)]
        kinds: Vec<EdgeKind>,
        direction: Direction,
    },
    Findings {
        #[serde(flatten)]
        filter: FindingFilter,
    },
    SymbolsInFile { path: String },
    EdgesByFile {
        path: String,
        #[serde(default)]
        kinds: Vec<EdgeKind>,
        direction: Direction,
    },
    UnresolvedCallers {
        names: Vec<String>,
        #[serde(default)]
        kinds: Vec<EdgeKind>,
    },
    ImportersOfFile { path: String },
    FilesAtPrefix { prefix: String },
    Metadata,
    /// Find symbols by qualified name. Exact `qualified_name = X` matches rank
    /// first; suffix matches (`*.X`) follow. Agents query by short name when
    /// they don't yet know the full module path.
    FindByQname { qname: String },
    /// Composite blast-radius lookup by qualified name. Picks the top
    /// `FindByQname` match and returns its callers, callees, and test seams
    /// in one shot — the agent-facing surface for "what breaks if I touch X?"
    BlastRadius { qname: String },
    /// Test seams targeting a symbol — inbound callers from files
    /// classified as test files by path or qname convention. Standalone
    /// version of the `BlastRadius.test_seams` slice; useful when the
    /// agent only needs to know "which tests exercise this symbol?"
    TestSeams { qname: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", content = "value")]
pub enum QueryResult {
    LookupSymbol(Option<SymbolRecord>),
    Neighbors(Vec<NeighborEdge>),
    Expand(Subgraph),
    Findings(Vec<FindingRecord>),
    SymbolsInFile(Vec<SymbolRecord>),
    EdgesByFile(Vec<FileEdgeBundle>),
    UnresolvedCallers(Vec<UnresolvedCallerHit>),
    ImportersOfFile(Vec<SymbolRecord>),
    FilesAtPrefix(Vec<FileRecordOut>),
    Metadata(MetadataRecord),
    FindByQname(Vec<SymbolRecord>),
    BlastRadius(Option<BlastRadius>),
    TestSeams(Vec<SymbolRecord>),
}

/// Verified handle to a built Index. `open` checks `index_format_version` once;
/// every method on `&self` reads from the same opened DuckDB connection.
pub struct IndexReader {
    conn: Connection,
}

impl IndexReader {
    pub fn open(path: &Path) -> Result<Self> {
        // `Connection::open` creates the file when missing, so a pre-check is the
        // only way to distinguish "absent index" from "empty index" cleanly.
        if !path.exists() {
            return Err(MallardError::IndexNotFound(path.to_path_buf()));
        }
        let conn = Connection::open(path)?;
        verify_format_version(&conn)?;
        Ok(IndexReader { conn })
    }

    pub fn metadata(&self) -> Result<MetadataRecord> {
        let sql = format!(
            "SELECT {k}, {v} FROM {t}",
            k = cols::metadata::KEY,
            v = cols::metadata::VALUE,
            t = tables::METADATA,
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = MetadataRecord {
            sha: None,
            indexer_version: None,
            rule_set_hash: None,
            built_at: None,
            language_allow_list: Vec::new(),
            index_format_version: schema::INDEX_FORMAT_VERSION,
        };
        for row in rows {
            let (k, v) = row?;
            match k.as_str() {
                metadata_keys::SHA => out.sha = Some(v),
                metadata_keys::INDEXER_VERSION => out.indexer_version = Some(v),
                metadata_keys::RULE_SET_HASH => out.rule_set_hash = Some(v),
                metadata_keys::BUILT_AT => out.built_at = Some(v),
                metadata_keys::LANGUAGE_ALLOW_LIST => {
                    out.language_allow_list = if v.is_empty() {
                        Vec::new()
                    } else {
                        v.split(',').map(str::to_string).collect()
                    };
                }
                metadata_keys::INDEX_FORMAT_VERSION => {
                    out.index_format_version = v.parse().unwrap_or(schema::INDEX_FORMAT_VERSION);
                }
                _ => {}
            }
        }
        Ok(out)
    }

    pub fn lookup_symbol(&self, id: &SymbolId) -> Result<Option<SymbolRecord>> {
        fetch_symbol(&self.conn, id)
    }

    pub fn symbols_in_file(&self, file_path: &str) -> Result<Vec<SymbolRecord>> {
        let sql = format!("{SYMBOL_SELECT} WHERE f.path = ? ORDER BY s.anchor_start_byte");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![file_path], map_symbol_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn files_at_prefix(&self, prefix: &str) -> Result<Vec<FileRecordOut>> {
        let sql = format!(
            "SELECT {fid}, {p}, {l}, {sz}, {st} FROM {t} WHERE {p} LIKE ? ESCAPE '\\' ORDER BY {p}",
            fid = cols::files::FILE_ID,
            p = cols::files::PATH,
            l = cols::files::LANGUAGE,
            sz = cols::files::SIZE_BYTES,
            st = cols::files::STATUS,
            t = tables::FILES,
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![like_escape(prefix)], |r| {
            let status_str: String = r.get(4)?;
            Ok(FileRecordOut {
                file_id: r.get(0)?,
                path: r.get(1)?,
                language: r.get::<_, Option<String>>(2)?,
                size_bytes: r.get::<_, i64>(3)? as u64,
                status: FileStatus::from_str(&status_str).unwrap_or(FileStatus::Indexed),
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn neighbors(
        &self,
        id: &SymbolId,
        kinds: &[EdgeKind],
        dir: Direction,
    ) -> Result<Vec<NeighborEdge>> {
        let kinds: Vec<EdgeKind> = if kinds.is_empty() {
            EdgeKind::all().to_vec()
        } else {
            kinds.to_vec()
        };
        neighbors_inner(
            &self.conn,
            id,
            &kinds,
            matches!(dir, Direction::Out | Direction::Both),
            matches!(dir, Direction::In | Direction::Both),
        )
    }

    pub fn expand(
        &self,
        id: &SymbolId,
        depth: u32,
        kinds: &[EdgeKind],
        dir: Direction,
    ) -> Result<Subgraph> {
        let anchor = match fetch_symbol(&self.conn, id)? {
            Some(s) => s,
            None => {
                return Ok(Subgraph {
                    nodes: Vec::new(),
                    edges: Vec::new(),
                    max_depth_reached: 0,
                });
            }
        };

        if depth == 0 {
            return Ok(Subgraph {
                nodes: vec![anchor],
                edges: Vec::new(),
                max_depth_reached: 0,
            });
        }

        let kinds_owned: Vec<EdgeKind> = if kinds.is_empty() {
            EdgeKind::all().to_vec()
        } else {
            kinds.to_vec()
        };
        let want_out = matches!(dir, Direction::Out | Direction::Both);
        let want_in = matches!(dir, Direction::In | Direction::Both);

        let mut visited_nodes: std::collections::BTreeMap<String, SymbolRecord> =
            std::collections::BTreeMap::new();
        visited_nodes.insert(anchor.id.0.clone(), anchor.clone());
        let mut all_edges: Vec<NeighborEdge> = Vec::new();
        let mut frontier: Vec<SymbolId> = vec![anchor.id.clone()];
        let mut reached: u32 = 0;

        for d in 1..=depth {
            if frontier.is_empty() {
                break;
            }
            let mut next: Vec<SymbolId> = Vec::new();
            for source_id in &frontier {
                let edges = neighbors_inner(&self.conn, source_id, &kinds_owned, want_out, want_in)?;
                for e in edges {
                    if let Some(dst) = &e.dst {
                        if !visited_nodes.contains_key(&dst.id.0) {
                            visited_nodes.insert(dst.id.0.clone(), dst.clone());
                            next.push(dst.id.clone());
                        }
                    }
                    if !visited_nodes.contains_key(&e.src.id.0) {
                        visited_nodes.insert(e.src.id.0.clone(), e.src.clone());
                    }
                    all_edges.push(e);
                }
            }
            if !next.is_empty() {
                reached = d;
            }
            frontier = next;
        }

        let nodes: Vec<SymbolRecord> = visited_nodes.into_values().collect();
        Ok(Subgraph {
            nodes,
            edges: all_edges,
            max_depth_reached: reached,
        })
    }

    pub fn findings(&self, filter: &FindingFilter) -> Result<Vec<FindingRecord>> {
        let mut symbol_anchor: Option<(FileId, u32, u32)> = None;
        if let Some(sid) = &filter.symbol_id {
            match fetch_symbol(&self.conn, sid)? {
                Some(s) => {
                    symbol_anchor = Some((s.file_id, s.anchor.start_line, s.anchor.end_line));
                }
                None => return Ok(Vec::new()),
            }
        }

        let mut where_parts: Vec<String> = Vec::new();
        let mut bound: Vec<Box<dyn duckdb::ToSql>> = Vec::new();
        if let Some(rule) = &filter.rule_id {
            where_parts.push("fnd.rule_id = ?".to_string());
            bound.push(Box::new(rule.clone()));
        }
        if let Some(prefix) = &filter.path_prefix {
            where_parts.push("f.path LIKE ? ESCAPE '\\'".to_string());
            bound.push(Box::new(like_escape(prefix)));
        }
        if let Some((fid, start, end)) = symbol_anchor {
            where_parts
                .push("fnd.file_id = ? AND fnd.end_line >= ? AND fnd.start_line <= ?".to_string());
            bound.push(Box::new(fid));
            bound.push(Box::new(start as i32));
            bound.push(Box::new(end as i32));
        }

        let where_clause = if where_parts.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", where_parts.join(" AND "))
        };
        let sql = format!(
            "SELECT fnd.rule_id, fnd.file_id, f.path, fnd.start_line, fnd.end_line, fnd.message \
             FROM findings fnd JOIN files f ON f.file_id = fnd.file_id{where_clause} \
             ORDER BY f.path, fnd.start_line, fnd.rule_id"
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let p_refs: Vec<&dyn duckdb::ToSql> =
            bound.iter().map(|b| &**b as &dyn duckdb::ToSql).collect();
        let rows = stmt.query_map(p_refs.as_slice(), |r| {
            Ok(FindingRecord {
                rule_id: r.get(0)?,
                file_id: r.get(1)?,
                path: r.get(2)?,
                start_line: r.get::<_, i32>(3)? as u32,
                end_line: r.get::<_, i32>(4)? as u32,
                message: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn run(&self, request: &QueryRequest) -> Result<QueryResult> {
        match request {
            QueryRequest::LookupSymbol { id } => {
                Ok(QueryResult::LookupSymbol(self.lookup_symbol(id)?))
            }
            QueryRequest::Neighbors { id, kinds, direction } => Ok(QueryResult::Neighbors(
                self.neighbors(id, kinds, *direction)?,
            )),
            QueryRequest::Expand {
                id,
                depth,
                kinds,
                direction,
            } => Ok(QueryResult::Expand(
                self.expand(id, *depth, kinds, *direction)?,
            )),
            QueryRequest::Findings { filter } => Ok(QueryResult::Findings(self.findings(filter)?)),
            QueryRequest::SymbolsInFile { path } => {
                Ok(QueryResult::SymbolsInFile(self.symbols_in_file(path)?))
            }
            QueryRequest::EdgesByFile {
                path,
                kinds,
                direction,
            } => Ok(QueryResult::EdgesByFile(
                self.edges_by_file(path, kinds, *direction)?,
            )),
            QueryRequest::UnresolvedCallers { names, kinds } => Ok(
                QueryResult::UnresolvedCallers(self.unresolved_callers(names, kinds)?),
            ),
            QueryRequest::ImportersOfFile { path } => {
                Ok(QueryResult::ImportersOfFile(self.importers_of_file(path)?))
            }
            QueryRequest::FilesAtPrefix { prefix } => {
                Ok(QueryResult::FilesAtPrefix(self.files_at_prefix(prefix)?))
            }
            QueryRequest::Metadata => Ok(QueryResult::Metadata(self.metadata()?)),
            QueryRequest::FindByQname { qname } => {
                Ok(QueryResult::FindByQname(self.find_by_qname(qname)?))
            }
            QueryRequest::BlastRadius { qname } => {
                Ok(QueryResult::BlastRadius(self.blast_radius(qname)?))
            }
            QueryRequest::TestSeams { qname } => {
                Ok(QueryResult::TestSeams(self.test_seams(qname)?))
            }
        }
    }

    /// Bulk per-file edges. One SQL query per active direction (each JOINs
    /// src + dst symbols + files so no per-row `fetch_symbol` round-trip);
    /// preserves symbols with no edges so callers can `comm`-diff bundles
    /// without re-querying.
    pub fn edges_by_file(
        &self,
        file_path: &str,
        kinds: &[EdgeKind],
        dir: Direction,
    ) -> Result<Vec<FileEdgeBundle>> {
        use std::collections::HashMap;

        let kinds: Vec<EdgeKind> = if kinds.is_empty() {
            EdgeKind::all().to_vec()
        } else {
            kinds.to_vec()
        };

        let symbols = self.symbols_in_file(file_path)?;
        if symbols.is_empty() {
            return Ok(Vec::new());
        }

        let mut order: Vec<SymbolId> = Vec::with_capacity(symbols.len());
        let mut bundles: HashMap<SymbolId, FileEdgeBundle> = HashMap::with_capacity(symbols.len());
        for s in symbols {
            order.push(s.id.clone());
            bundles.insert(
                s.id.clone(),
                FileEdgeBundle {
                    symbol: s,
                    outbound: Vec::new(),
                    inbound: Vec::new(),
                },
            );
        }

        if matches!(dir, Direction::Out | Direction::Both) {
            self.load_outbound(file_path, &kinds, &mut bundles)?;
        }
        if matches!(dir, Direction::In | Direction::Both) {
            self.load_inbound(file_path, &kinds, &mut bundles)?;
        }

        Ok(order
            .into_iter()
            .filter_map(|id| bundles.remove(&id))
            .collect())
    }

    fn load_outbound(
        &self,
        file_path: &str,
        kinds: &[EdgeKind],
        bundles: &mut std::collections::HashMap<SymbolId, FileEdgeBundle>,
    ) -> Result<()> {
        let placeholders = vec!["?"; kinds.len()].join(",");
        let sql = format!(
            "SELECT s.symbol_id, \
                    e.kind, e.confidence, e.dst_symbol_id, e.dst_unresolved, \
                    dst.symbol_id, dst.file_id, dst_f.path, dst.qualified_name, dst.kind, dst.signature, \
                    dst.anchor_start_byte, dst.anchor_end_byte, dst.anchor_start_line, dst.anchor_end_line \
             FROM symbols s \
             JOIN files f ON f.file_id = s.file_id \
             JOIN edges e ON e.src_symbol_id = s.symbol_id \
             LEFT JOIN symbols dst ON dst.symbol_id = e.dst_symbol_id \
             LEFT JOIN files dst_f ON dst_f.file_id = dst.file_id \
             WHERE f.path = ? AND e.kind IN ({placeholders}) \
             ORDER BY s.anchor_start_byte, e.edge_id"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let params = bind_strings_and_kinds(&[file_path], kinds);
        let p_refs: Vec<&dyn duckdb::ToSql> =
            params.iter().map(|b| &**b as &dyn duckdb::ToSql).collect();
        let rows = stmt.query_map(p_refs.as_slice(), |r| {
            Ok((
                r.get::<_, String>(0)?,         // src symbol_id
                r.get::<_, String>(1)?,         // edge kind
                r.get::<_, String>(2)?,         // confidence
                r.get::<_, Option<String>>(4)?, // dst_unresolved
                read_optional_symbol(r, 5)?,
            ))
        })?;
        for row in rows {
            let (src_id, kind_s, conf_s, dst_unresolved, dst_rec) = row?;
            let src_id = SymbolId(src_id);
            let src_rec = match bundles.get(&src_id) {
                Some(b) => b.symbol.clone(),
                None => continue,
            };
            let bundle = bundles
                .get_mut(&src_id)
                .expect("symbol_id matches a bundle in this file");
            bundle.outbound.push(NeighborEdge {
                kind: EdgeKind::from_str(&kind_s).unwrap_or(EdgeKind::Calls),
                confidence: EdgeConfidence::from_str(&conf_s)
                    .unwrap_or(EdgeConfidence::Unresolved),
                direction: Direction::Out,
                src: src_rec,
                dst: dst_rec,
                dst_unresolved,
            });
        }
        Ok(())
    }

    fn load_inbound(
        &self,
        file_path: &str,
        kinds: &[EdgeKind],
        bundles: &mut std::collections::HashMap<SymbolId, FileEdgeBundle>,
    ) -> Result<()> {
        let placeholders = vec!["?"; kinds.len()].join(",");
        let sql = format!(
            "SELECT s.symbol_id, \
                    e.kind, e.confidence, \
                    src.symbol_id, src.file_id, src_f.path, src.qualified_name, src.kind, src.signature, \
                    src.anchor_start_byte, src.anchor_end_byte, src.anchor_start_line, src.anchor_end_line \
             FROM symbols s \
             JOIN files f ON f.file_id = s.file_id \
             JOIN edges e ON e.dst_symbol_id = s.symbol_id \
             JOIN symbols src ON src.symbol_id = e.src_symbol_id \
             JOIN files src_f ON src_f.file_id = src.file_id \
             WHERE f.path = ? AND e.kind IN ({placeholders}) \
             ORDER BY s.anchor_start_byte, e.edge_id"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let params = bind_strings_and_kinds(&[file_path], kinds);
        let p_refs: Vec<&dyn duckdb::ToSql> =
            params.iter().map(|b| &**b as &dyn duckdb::ToSql).collect();
        let rows = stmt.query_map(p_refs.as_slice(), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                read_required_symbol(r, 3)?,
            ))
        })?;
        for row in rows {
            let (dst_id, kind_s, conf_s, src_rec) = row?;
            let dst_id = SymbolId(dst_id);
            let dst_rec = match bundles.get(&dst_id) {
                Some(b) => b.symbol.clone(),
                None => continue,
            };
            let bundle = bundles
                .get_mut(&dst_id)
                .expect("symbol_id matches a bundle in this file");
            bundle.inbound.push(NeighborEdge {
                kind: EdgeKind::from_str(&kind_s).unwrap_or(EdgeKind::Calls),
                confidence: EdgeConfidence::from_str(&conf_s)
                    .unwrap_or(EdgeConfidence::Unresolved),
                direction: Direction::In,
                src: src_rec,
                dst: Some(dst_rec),
                dst_unresolved: None,
            });
        }
        Ok(())
    }

    /// Find every edge whose `dst_unresolved` matches any of the given names
    /// (typically the short names of symbols removed in a PR). One SQL query
    /// joins the edges table against the symbols + files tables for caller
    /// enrichment. Empty `names` returns `[]` without querying.
    pub fn unresolved_callers(
        &self,
        names: &[String],
        kinds: &[EdgeKind],
    ) -> Result<Vec<UnresolvedCallerHit>> {
        if names.is_empty() {
            return Ok(Vec::new());
        }
        let kinds: Vec<EdgeKind> = if kinds.is_empty() {
            EdgeKind::all().to_vec()
        } else {
            kinds.to_vec()
        };
        let name_placeholders = vec!["?"; names.len()].join(",");
        let kind_placeholders = vec!["?"; kinds.len()].join(",");
        let sql = format!(
            "SELECT e.dst_unresolved, e.kind, e.confidence, \
                    src.symbol_id, src.file_id, sf.path, src.qualified_name, src.kind, src.signature, \
                    src.anchor_start_byte, src.anchor_end_byte, src.anchor_start_line, src.anchor_end_line \
             FROM edges e \
             JOIN symbols src ON src.symbol_id = e.src_symbol_id \
             JOIN files sf ON sf.file_id = src.file_id \
             WHERE e.dst_symbol_id IS NULL \
               AND e.dst_unresolved IN ({name_placeholders}) \
               AND e.kind IN ({kind_placeholders}) \
             ORDER BY sf.path, src.anchor_start_byte"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        let params = bind_strings_and_kinds(&name_refs, &kinds);
        let p_refs: Vec<&dyn duckdb::ToSql> =
            params.iter().map(|b| &**b as &dyn duckdb::ToSql).collect();
        let rows = stmt.query_map(p_refs.as_slice(), |r| {
            Ok(UnresolvedCallerHit {
                unresolved_name: r.get::<_, String>(0)?,
                edge_kind: EdgeKind::from_str(&r.get::<_, String>(1)?)
                    .unwrap_or(EdgeKind::Calls),
                confidence: EdgeConfidence::from_str(&r.get::<_, String>(2)?)
                    .unwrap_or(EdgeConfidence::Unresolved),
                caller: read_required_symbol(r, 3)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Find symbols by qualified name. Exact `qualified_name = qname` matches
    /// rank first, then suffix matches across both separator conventions —
    /// `*.qname` (Python / TS / JS) and `*::qname` (Rust). Within each tier,
    /// results are ordered by path then anchor. Agents typically query a
    /// short name (e.g. `foo`) and rely on suffix matching to find symbols
    /// defined as `module.bar.foo` or `Bar::foo`.
    pub fn find_by_qname(&self, qname: &str) -> Result<Vec<SymbolRecord>> {
        let dot_suffix = format!("%.{qname}");
        let colon_suffix = format!("%::{qname}");
        let sql = format!(
            "{SYMBOL_SELECT} WHERE s.qualified_name = ? \
                OR s.qualified_name LIKE ? \
                OR s.qualified_name LIKE ? \
             ORDER BY (s.qualified_name = ?) DESC, f.path, s.anchor_start_byte"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(
            params![qname, dot_suffix, colon_suffix, qname],
            map_symbol_row,
        )?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Composite blast-radius: qname → top match → inbound + outbound
    /// neighbors (all edge kinds) → split into callers/callees/test_seams.
    /// Test classification uses `pr_review::is_test_symbol` (path conventions
    /// + qname prefixes), so Rust `mod tests { ... }` is covered without a
    /// `tests/` directory. Returns `None` only when the qname matches no
    /// symbol; an isolated symbol with no neighbors yields a
    /// `BlastRadius` with empty caller/callee lists.
    pub fn blast_radius(&self, qname: &str) -> Result<Option<BlastRadius>> {
        let matches = self.find_by_qname(qname)?;
        let Some(symbol) = matches.first().cloned() else {
            return Ok(None);
        };
        let other_qname_matches = matches.into_iter().skip(1).collect();

        let edges = self.neighbors(&symbol.id, &[], Direction::Both)?;
        let mut callers: Vec<SymbolRecord> = Vec::new();
        let mut callees: Vec<SymbolRecord> = Vec::new();
        let mut test_seams: Vec<SymbolRecord> = Vec::new();
        let mut seen_callers = std::collections::HashSet::new();
        let mut seen_callees = std::collections::HashSet::new();
        let mut seen_seams = std::collections::HashSet::new();

        for e in edges {
            match e.direction {
                Direction::In => {
                    let caller = e.src;
                    if seen_callers.insert(caller.id.0.clone()) {
                        if crate::pr_review::is_test_symbol(
                            &caller.path,
                            Some(&caller.qualified_name),
                        ) && seen_seams.insert(caller.id.0.clone())
                        {
                            test_seams.push(caller.clone());
                        }
                        callers.push(caller);
                    }
                }
                Direction::Out => {
                    if let Some(callee) = e.dst
                        && seen_callees.insert(callee.id.0.clone()) {
                        callees.push(callee);
                    }
                }
                Direction::Both => {}
            }
        }

        Ok(Some(BlastRadius {
            symbol,
            callers,
            callees,
            test_seams,
            other_qname_matches,
        }))
    }

    /// Standalone test-seam lookup. Resolves qname → symbol → inbound
    /// neighbors, then filters callers by `pr_review::is_test_symbol`.
    /// Returns empty vec if qname has no match (so it composes cleanly
    /// with shell pipelines that don't want a null result).
    pub fn test_seams(&self, qname: &str) -> Result<Vec<SymbolRecord>> {
        let matches = self.find_by_qname(qname)?;
        let Some(symbol) = matches.first() else {
            return Ok(Vec::new());
        };
        let edges = self.neighbors(&symbol.id, &[], Direction::In)?;
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for e in edges {
            let caller = e.src;
            if crate::pr_review::is_test_symbol(&caller.path, Some(&caller.qualified_name))
                && seen.insert(caller.id.0.clone())
            {
                out.push(caller);
            }
        }
        Ok(out)
    }

    /// Stream every indexed symbol with file path enrichment. Ordered by
    /// `(path, anchor_start_byte)` for deterministic output. Backs
    /// `symbol_diff` and ad-hoc full-index dumps.
    pub fn all_symbols(&self) -> Result<Vec<SymbolRecord>> {
        let sql = format!("{SYMBOL_SELECT} ORDER BY f.path, s.anchor_start_byte");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], map_symbol_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn importers_of_file(&self, file_path: &str) -> Result<Vec<SymbolRecord>> {
        let sql = format!(
            "SELECT DISTINCT src.symbol_id, src.file_id, src_f.path, src.qualified_name, src.kind, src.signature, \
                    src.anchor_start_byte, src.anchor_end_byte, src.anchor_start_line, src.anchor_end_line \
             FROM edges e \
             JOIN symbols src ON src.symbol_id = e.src_symbol_id \
             JOIN files src_f ON src_f.file_id = src.file_id \
             JOIN symbols dst ON dst.symbol_id = e.dst_symbol_id \
             JOIN files dst_f ON dst_f.file_id = dst.file_id \
             WHERE e.kind = ? AND dst_f.path = ? \
             ORDER BY src_f.path, src.anchor_start_byte"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows =
            stmt.query_map(params![EdgeKind::Imports.as_str(), file_path], map_symbol_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}

fn bind_strings_and_kinds(
    leading: &[&str],
    kinds: &[EdgeKind],
) -> Vec<Box<dyn duckdb::ToSql>> {
    let mut p: Vec<Box<dyn duckdb::ToSql>> = Vec::with_capacity(leading.len() + kinds.len());
    for s in leading {
        p.push(Box::new(s.to_string()));
    }
    for k in kinds {
        p.push(Box::new(k.as_str().to_string()));
    }
    p
}

/// Read a 10-column block as a SymbolRecord starting at column `base`.
/// Layout matches the `dst.* / dst_f.path` ordering used in JOIN queries:
/// (symbol_id, file_id, path, qualified_name, kind, signature, anchor_start_byte,
///  anchor_end_byte, anchor_start_line, anchor_end_line).
fn read_required_symbol(row: &duckdb::Row<'_>, base: usize) -> duckdb::Result<SymbolRecord> {
    let kind_str: String = row.get(base + 4)?;
    Ok(SymbolRecord {
        id: SymbolId(row.get(base)?),
        file_id: row.get(base + 1)?,
        path: row.get(base + 2)?,
        qualified_name: row.get(base + 3)?,
        kind: SymbolKind::from_str(&kind_str).unwrap_or(SymbolKind::Other),
        signature: row.get::<_, Option<String>>(base + 5)?.unwrap_or_default(),
        anchor: Anchor {
            start_byte: row.get::<_, i64>(base + 6)? as u64,
            end_byte: row.get::<_, i64>(base + 7)? as u64,
            start_line: row.get::<_, i32>(base + 8)? as u32,
            end_line: row.get::<_, i32>(base + 9)? as u32,
        },
    })
}

/// Same layout as `read_required_symbol`, but the JOIN may have produced
/// NULL columns (LEFT JOIN on edges → symbols). Returns None when the
/// peer symbol_id is NULL.
fn read_optional_symbol(row: &duckdb::Row<'_>, base: usize) -> duckdb::Result<Option<SymbolRecord>> {
    let id: Option<String> = row.get(base)?;
    let Some(id) = id else { return Ok(None) };
    let kind_str: String = row.get(base + 4)?;
    Ok(Some(SymbolRecord {
        id: SymbolId(id),
        file_id: row.get(base + 1)?,
        path: row.get(base + 2)?,
        qualified_name: row.get(base + 3)?,
        kind: SymbolKind::from_str(&kind_str).unwrap_or(SymbolKind::Other),
        signature: row.get::<_, Option<String>>(base + 5)?.unwrap_or_default(),
        anchor: Anchor {
            start_byte: row.get::<_, i64>(base + 6)? as u64,
            end_byte: row.get::<_, i64>(base + 7)? as u64,
            start_line: row.get::<_, i32>(base + 8)? as u32,
            end_line: row.get::<_, i32>(base + 9)? as u32,
        },
    }))
}

fn verify_format_version(conn: &Connection) -> Result<()> {
    let sql = format!(
        "SELECT {v} FROM {t} WHERE {k} = ?",
        v = cols::metadata::VALUE,
        t = tables::METADATA,
        k = cols::metadata::KEY,
    );
    let found: Option<String> = conn
        .query_row(&sql, params![metadata_keys::INDEX_FORMAT_VERSION], |r| {
            r.get::<_, String>(0)
        })
        .optional()?;
    let Some(found) = found else {
        return Err(MallardError::VersionMismatch {
            found: 0,
            expected: schema::INDEX_FORMAT_VERSION,
        });
    };
    let found: u32 = found.parse().map_err(|_| MallardError::VersionMismatch {
        found: 0,
        expected: schema::INDEX_FORMAT_VERSION,
    })?;
    if found != schema::INDEX_FORMAT_VERSION {
        return Err(MallardError::VersionMismatch {
            found,
            expected: schema::INDEX_FORMAT_VERSION,
        });
    }
    Ok(())
}

const SYMBOL_SELECT: &str = "\
SELECT s.symbol_id, s.file_id, f.path, s.qualified_name, s.kind, s.signature, \
       s.anchor_start_byte, s.anchor_end_byte, s.anchor_start_line, s.anchor_end_line \
FROM symbols s JOIN files f ON f.file_id = s.file_id";

fn map_symbol_row(row: &duckdb::Row<'_>) -> duckdb::Result<SymbolRecord> {
    let kind_str: String = row.get(4)?;
    Ok(SymbolRecord {
        id: SymbolId(row.get(0)?),
        file_id: row.get(1)?,
        path: row.get(2)?,
        qualified_name: row.get(3)?,
        kind: SymbolKind::from_str(&kind_str).unwrap_or(SymbolKind::Other),
        signature: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
        anchor: Anchor {
            start_byte: row.get::<_, i64>(6)? as u64,
            end_byte: row.get::<_, i64>(7)? as u64,
            start_line: row.get::<_, i32>(8)? as u32,
            end_line: row.get::<_, i32>(9)? as u32,
        },
    })
}

fn like_escape(prefix: &str) -> String {
    format!("{}%", prefix.replace('%', "\\%").replace('_', "\\_"))
}

fn fetch_symbol(conn: &Connection, id: &SymbolId) -> Result<Option<SymbolRecord>> {
    let sql = format!("{SYMBOL_SELECT} WHERE s.symbol_id = ?");
    let mut stmt = conn.prepare(&sql)?;
    let row = stmt
        .query_row(params![id.as_str()], map_symbol_row)
        .optional()?;
    Ok(row)
}

// `src` may be a pseudo-id like `file:<path>` (build-side artifact for Contains
// edges that originate at a file rather than a symbol). Such edges are skipped:
// query-side expansion operates over real symbols only.
fn neighbors_inner(
    conn: &Connection,
    id: &SymbolId,
    kinds: &[EdgeKind],
    want_out: bool,
    want_in: bool,
) -> Result<Vec<NeighborEdge>> {
    let placeholders = vec!["?"; kinds.len()].join(",");
    let mut out: Vec<NeighborEdge> = Vec::new();
    let active: &[(&str, Direction)] = match (want_out, want_in) {
        (true, true) => &[
            ("src_symbol_id", Direction::Out),
            ("dst_symbol_id", Direction::In),
        ],
        (true, false) => &[("src_symbol_id", Direction::Out)],
        (false, true) => &[("dst_symbol_id", Direction::In)],
        (false, false) => &[],
    };

    for (col, direction) in active {
        let sql = format!(
            "SELECT e.src_symbol_id, e.dst_symbol_id, e.dst_unresolved, e.kind, e.confidence \
             FROM edges e \
             WHERE e.{col} = ? AND e.kind IN ({placeholders}) \
             ORDER BY e.edge_id"
        );
        let mut stmt = conn.prepare(&sql)?;
        let params = bind_strings_and_kinds(&[id.as_str()], kinds);
        let p_refs: Vec<&dyn duckdb::ToSql> =
            params.iter().map(|b| &**b as &dyn duckdb::ToSql).collect();
        let rows = stmt.query_map(p_refs.as_slice(), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;
        for row in rows {
            let (src, dst, unresolved, kind_s, conf_s) = row?;
            let Some(src_rec) = fetch_symbol(conn, &SymbolId(src))? else {
                continue;
            };
            let dst_rec = match &dst {
                Some(s) => fetch_symbol(conn, &SymbolId(s.clone()))?,
                None => None,
            };
            out.push(NeighborEdge {
                kind: EdgeKind::from_str(&kind_s).unwrap_or(EdgeKind::Calls),
                confidence: EdgeConfidence::from_str(&conf_s).unwrap_or(EdgeConfidence::Unresolved),
                direction: *direction,
                src: src_rec,
                dst: dst_rec,
                dst_unresolved: unresolved,
            });
        }
    }
    Ok(out)
}
