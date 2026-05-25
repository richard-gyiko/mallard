use std::collections::HashMap;
use std::path::{Path, PathBuf};

use duckdb::{Connection, params};

use std::str::FromStr;

use crate::core::{
    Anchor, Edge, EdgeKind, FileId, FileRecord, Finding, MallardError, Metadata, ParseError,
    ParsedFile, Result, Symbol, SymbolKind,
};
use crate::schema::{self, cols, metadata_keys, tables};

/// Counts produced by post-write resolution. Logged by the caller.
#[derive(Debug, Clone, Default)]
pub struct ResolveStats {
    pub inspected: u64,
    pub resolved: u64,
    pub ambiguous: u64,
}

enum Match {
    Unique(String),
    Ambiguous,
    None,
}

pub struct IndexWriter {
    conn: Connection,
    final_path: PathBuf,
    tmp_path: PathBuf,
}

impl IndexWriter {
    pub fn create(final_path: &Path, meta: &Metadata) -> Result<Self> {
        if let Some(parent) = final_path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let tmp_path = tmp_path_for(final_path);
        match std::fs::remove_file(&tmp_path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        let conn = Connection::open(&tmp_path)?;
        conn.execute_batch(schema::DDL)?;
        seed_metadata(&conn, meta)?;
        Ok(IndexWriter {
            conn,
            final_path: final_path.to_path_buf(),
            tmp_path,
        })
    }

    pub fn write_indexed(
        &mut self,
        file: &FileRecord,
        parsed: &ParsedFile,
        findings: &[Finding],
    ) -> Result<()> {
        self.append_file(file)?;
        self.append_symbols(parsed.file_id, &parsed.symbols)?;
        self.append_edges(&parsed.edges)?;
        self.append_findings(findings)?;
        self.append_parse_errors(&parsed.parse_errors)?;
        Ok(())
    }

    pub fn write_skipped(&mut self, file: &FileRecord) -> Result<()> {
        self.append_file(file)
    }

    fn append_file(&mut self, file: &FileRecord) -> Result<()> {
        let mut app = self.conn.appender(tables::FILES)?;
        app.append_row(params![
            file.id,
            file.path.as_str(),
            file.language.as_deref(),
            file.size_bytes as i64,
            file.status.as_str(),
        ])?;
        app.flush()?;
        Ok(())
    }

    fn append_symbols(&mut self, file_id: FileId, syms: &[Symbol]) -> Result<()> {
        if syms.is_empty() {
            return Ok(());
        }
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let mut app = self.conn.appender(tables::SYMBOLS)?;
        for sym in syms {
            if !seen.insert(sym.id.as_str()) {
                tracing::warn!(
                    symbol_id = sym.id.as_str(),
                    qualified_name = sym.qualified_name.as_str(),
                    kind = sym.kind.as_str(),
                    file_id,
                    "dropping duplicate symbol id within file"
                );
                continue;
            }
            let a: Anchor = sym.anchor;
            app.append_row(params![
                sym.id.as_str(),
                file_id,
                sym.qualified_name.as_str(),
                sym.kind.as_str(),
                sym.signature.as_str(),
                a.start_byte as i64,
                a.end_byte as i64,
                a.start_line as i32,
                a.end_line as i32,
            ])?;
        }
        app.flush()?;
        Ok(())
    }

    fn append_edges(&mut self, edges: &[Edge]) -> Result<()> {
        if edges.is_empty() {
            return Ok(());
        }
        let mut app = self.conn.appender_with_columns(
            tables::EDGES,
            &[
                cols::edges::SRC_SYMBOL_ID,
                cols::edges::DST_SYMBOL_ID,
                cols::edges::DST_UNRESOLVED,
                cols::edges::KIND,
                cols::edges::CONFIDENCE,
                cols::edges::FILE_ID,
            ],
        )?;
        for edge in edges {
            app.append_row(params![
                edge.src.as_str(),
                edge.dst.as_ref().map(|d| d.as_str()),
                edge.dst_unresolved.as_deref(),
                edge.kind.as_str(),
                edge.confidence.as_str(),
                edge.file_id,
            ])?;
        }
        app.flush()?;
        Ok(())
    }

    fn append_findings(&mut self, findings: &[Finding]) -> Result<()> {
        if findings.is_empty() {
            return Ok(());
        }
        let mut app = self.conn.appender_with_columns(
            tables::FINDINGS,
            &[
                cols::findings::RULE_ID,
                cols::findings::FILE_ID,
                cols::findings::START_LINE,
                cols::findings::END_LINE,
                cols::findings::MESSAGE,
            ],
        )?;
        for f in findings {
            app.append_row(params![
                f.rule_id.as_str(),
                f.file_id,
                f.start_line as i32,
                f.end_line as i32,
                f.message.as_str(),
            ])?;
        }
        app.flush()?;
        Ok(())
    }

    fn append_parse_errors(&mut self, errs: &[ParseError]) -> Result<()> {
        if errs.is_empty() {
            return Ok(());
        }
        let mut app = self.conn.appender(tables::PARSE_ERRORS)?;
        for err in errs {
            app.append_row(params![
                err.file_id,
                err.message.as_str(),
                err.line as i32,
                err.col as i32,
            ])?;
        }
        app.flush()?;
        Ok(())
    }

    /// Resolve `dst_unresolved` on `calls` edges by matching against
    /// the global symbol name table. Among candidates with the same
    /// name, callable kinds (Function / Method / Macro) win; if exactly
    /// one callable matches a name, that's the resolution. Otherwise
    /// the edge stays unresolved.
    pub fn resolve_edges(&mut self) -> Result<ResolveStats> {
        let mut by_qualified: HashMap<String, Vec<(String, SymbolKind)>> = HashMap::new();
        let mut by_short: HashMap<String, Vec<(String, SymbolKind)>> = HashMap::new();
        {
            let mut stmt = self
                .conn
                .prepare("SELECT symbol_id, qualified_name, kind FROM symbols")?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?;
            for row in rows {
                let (sid, qname, kind_str) = row?;
                let kind = SymbolKind::from_str(&kind_str).unwrap_or(SymbolKind::Other);
                by_qualified
                    .entry(qname.clone())
                    .or_default()
                    .push((sid.clone(), kind));
                let short = short_name_for_resolver(&qname);
                by_short.entry(short).or_default().push((sid, kind));
            }
        }

        let mut pending: Vec<(i64, String)> = Vec::new();
        {
            // Only promote parser-tier `Unresolved` edges. Edges already at
            // `Ambiguous` were marked so by the parser (e.g. macro-body
            // method-position calls whose receiver type is unknown — see
            // `emit_macro_body_call` in extractor.rs) and must not be
            // re-tiered against the global short-name table; that would
            // falsely promote them to Inferred against any single
            // globally-unique unrelated symbol.
            let mut stmt = self.conn.prepare(
                "SELECT edge_id, dst_unresolved FROM edges \
                 WHERE dst_symbol_id IS NULL \
                   AND dst_unresolved IS NOT NULL \
                   AND kind = ? \
                   AND confidence = 'unresolved'",
            )?;
            let rows = stmt.query_map(params![EdgeKind::Calls.as_str()], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })?;
            for row in rows {
                pending.push(row?);
            }
        }

        let mut resolutions: Vec<(i64, String)> = Vec::new();
        let mut ambiguous: Vec<i64> = Vec::new();
        for (edge_id, name) in &pending {
            match classify_match(
                by_qualified.get(name).map(Vec::as_slice),
                by_short.get(name).map(Vec::as_slice),
            ) {
                Match::Unique(sid) => resolutions.push((*edge_id, sid)),
                Match::Ambiguous => ambiguous.push(*edge_id),
                Match::None => {}
            }
        }

        let resolved = resolutions.len() as u64;
        let ambiguous_count = ambiguous.len() as u64;
        if !resolutions.is_empty() {
            self.conn.execute_batch(
                "CREATE TEMPORARY TABLE _edge_resolutions (edge_id BIGINT, resolved_id VARCHAR)",
            )?;
            {
                let mut app = self.conn.appender("_edge_resolutions")?;
                for (eid, sid) in &resolutions {
                    app.append_row(params![*eid, sid.as_str()])?;
                }
                app.flush()?;
            }
            self.conn.execute(
                "UPDATE edges SET dst_symbol_id = r.resolved_id, dst_unresolved = NULL, \
                                  confidence = 'inferred' \
                 FROM _edge_resolutions r WHERE edges.edge_id = r.edge_id",
                [],
            )?;
            self.conn.execute_batch("DROP TABLE _edge_resolutions")?;
        }
        if !ambiguous.is_empty() {
            self.conn
                .execute_batch("CREATE TEMPORARY TABLE _edge_ambiguous (edge_id BIGINT)")?;
            {
                let mut app = self.conn.appender("_edge_ambiguous")?;
                for eid in &ambiguous {
                    app.append_row(params![*eid])?;
                }
                app.flush()?;
            }
            self.conn.execute(
                "UPDATE edges SET confidence = 'ambiguous' \
                 FROM _edge_ambiguous a WHERE edges.edge_id = a.edge_id",
                [],
            )?;
            self.conn.execute_batch("DROP TABLE _edge_ambiguous")?;
        }

        Ok(ResolveStats {
            inspected: pending.len() as u64,
            resolved,
            ambiguous: ambiguous_count,
        })
    }

    pub fn finalize(self) -> Result<()> {
        self.conn
            .close()
            .map_err(|(_, e)| MallardError::DuckDb(e))?;
        match std::fs::remove_file(&self.final_path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        std::fs::rename(&self.tmp_path, &self.final_path)?;
        Ok(())
    }
}

/// Short-name extraction for the resolver's global `by_short` table.
/// Handles both Rust's `Foo::bar` and Python's `Foo.bar` qualified names
/// — picks the rightmost segment after either separator. Used as the
/// resolver's bridge across language conventions; per-language symbol IDs
/// stay distinct because `qualified_name` keeps the original separator.
fn short_name_for_resolver(qname: &str) -> String {
    // Pick whichever separator ends latest in the string and slice past it.
    [
        qname.rfind("::").map(|i| i + 2),
        qname.rfind('.').map(|i| i + 1),
    ]
    .into_iter()
    .flatten()
    .max()
    .map(|i| qname[i..].to_string())
    .unwrap_or_else(|| qname.to_string())
}

fn classify_match(
    qualified: Option<&[(String, SymbolKind)]>,
    short: Option<&[(String, SymbolKind)]>,
) -> Match {
    if let Some(m) = pick_callable_match(qualified) {
        return m;
    }
    pick_callable_match(short).unwrap_or(Match::None)
}

fn pick_callable_match(candidates: Option<&[(String, SymbolKind)]>) -> Option<Match> {
    let candidates = candidates?;
    let callables: Vec<&String> = candidates
        .iter()
        .filter(|(_, k)| {
            matches!(
                k,
                SymbolKind::Function | SymbolKind::Method | SymbolKind::Macro
            )
        })
        .map(|(s, _)| s)
        .collect();
    Some(match callables.len() {
        0 => return None,
        1 => Match::Unique(callables[0].clone()),
        _ => Match::Ambiguous,
    })
}

fn tmp_path_for(final_path: &Path) -> PathBuf {
    let mut s = final_path.as_os_str().to_owned();
    s.push(".tmp");
    PathBuf::from(s)
}

fn seed_metadata(conn: &Connection, meta: &Metadata) -> Result<()> {
    let sql = format!(
        "INSERT INTO {} ({}, {}) VALUES (?, ?)",
        tables::METADATA,
        cols::metadata::KEY,
        cols::metadata::VALUE,
    );
    let mut stmt = conn.prepare(&sql)?;
    stmt.execute(params![metadata_keys::SHA, meta.sha.as_str()])?;
    stmt.execute(params![
        metadata_keys::INDEXER_VERSION,
        meta.indexer_version.as_str()
    ])?;
    if let Some(h) = &meta.rule_set_hash {
        stmt.execute(params![metadata_keys::RULE_SET_HASH, h.as_str()])?;
    }
    stmt.execute(params![metadata_keys::BUILT_AT, meta.built_at.as_str()])?;
    stmt.execute(params![
        metadata_keys::LANGUAGE_ALLOW_LIST,
        meta.language_allow_list.join(",").as_str()
    ])?;
    stmt.execute(params![
        metadata_keys::INDEX_FORMAT_VERSION,
        schema::INDEX_FORMAT_VERSION.to_string().as_str()
    ])?;
    Ok(())
}
