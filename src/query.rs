use std::path::Path;
use std::str::FromStr;

use duckdb::{Connection, OptionalExt, params};
use serde::{Deserialize, Serialize};

use crate::core::{
    Anchor, EdgeKind, FileId, FileStatus, MallardError, Result, SymbolId, SymbolKind,
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

    pub fn findings(&self, filter: FindingFilter) -> Result<Vec<FindingRecord>> {
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
            "SELECT e.src_symbol_id, e.dst_symbol_id, e.dst_unresolved, e.kind \
             FROM edges e \
             WHERE e.{col} = ? AND e.kind IN ({placeholders}) \
             ORDER BY e.edge_id"
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut p: Vec<Box<dyn duckdb::ToSql>> = Vec::with_capacity(kinds.len() + 1);
        p.push(Box::new(id.as_str().to_string()));
        for k in kinds {
            p.push(Box::new(k.as_str().to_string()));
        }
        let p_refs: Vec<&dyn duckdb::ToSql> =
            p.iter().map(|b| &**b as &dyn duckdb::ToSql).collect();
        let rows = stmt.query_map(p_refs.as_slice(), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        for row in rows {
            let (src, dst, unresolved, kind_s) = row?;
            let Some(src_rec) = fetch_symbol(conn, &SymbolId(src))? else {
                continue;
            };
            let dst_rec = match &dst {
                Some(s) => fetch_symbol(conn, &SymbolId(s.clone()))?,
                None => None,
            };
            out.push(NeighborEdge {
                kind: EdgeKind::from_str(&kind_s).unwrap_or(EdgeKind::References),
                direction: *direction,
                src: src_rec,
                dst: dst_rec,
                dst_unresolved: unresolved,
            });
        }
    }
    Ok(out)
}
