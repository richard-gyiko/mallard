use std::path::{Path, PathBuf};

use duckdb::{Connection, params};

use crate::core::{
    Anchor, Edge, EdgeKind, FileId, FileRecord, FileStatus, Finding, Metadata, MallardError,
    ParseError, Result, Symbol, SymbolKind,
};

const SCHEMA_DDL: &str = r#"
CREATE TABLE files (
    file_id      BIGINT PRIMARY KEY,
    path         VARCHAR NOT NULL,
    language     VARCHAR,
    size_bytes   BIGINT,
    status       VARCHAR
);

CREATE TABLE symbols (
    symbol_id         VARCHAR PRIMARY KEY,
    file_id           BIGINT NOT NULL,
    qualified_name    VARCHAR NOT NULL,
    kind              VARCHAR NOT NULL,
    signature         VARCHAR,
    anchor_start_byte BIGINT,
    anchor_end_byte   BIGINT,
    anchor_start_line INTEGER,
    anchor_end_line   INTEGER
);

CREATE SEQUENCE seq_edge_id START 1;
CREATE TABLE edges (
    edge_id        BIGINT PRIMARY KEY DEFAULT nextval('seq_edge_id'),
    src_symbol_id  VARCHAR NOT NULL,
    dst_symbol_id  VARCHAR,
    dst_unresolved VARCHAR,
    kind           VARCHAR NOT NULL,
    file_id        BIGINT NOT NULL
);

CREATE TABLE parse_errors (
    file_id BIGINT NOT NULL,
    message VARCHAR NOT NULL,
    line    INTEGER,
    col     INTEGER
);

CREATE SEQUENCE seq_finding_id START 1;
CREATE TABLE findings (
    finding_id BIGINT PRIMARY KEY DEFAULT nextval('seq_finding_id'),
    rule_id    VARCHAR NOT NULL,
    file_id    BIGINT NOT NULL,
    start_line INTEGER,
    end_line   INTEGER,
    message    VARCHAR
);

CREATE TABLE metadata (
    key   VARCHAR PRIMARY KEY,
    value VARCHAR
);
"#;

pub struct IndexWriter {
    conn: Connection,
    final_path: PathBuf,
    tmp_path: PathBuf,
}

impl IndexWriter {
    pub fn create(final_path: &Path, meta: &Metadata) -> Result<Self> {
        if let Some(parent) = final_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let tmp_path = tmp_path_for(final_path);
        if tmp_path.exists() {
            std::fs::remove_file(&tmp_path)?;
        }
        let conn = Connection::open(&tmp_path)?;
        conn.execute_batch(SCHEMA_DDL)?;
        seed_metadata(&conn, meta)?;
        Ok(IndexWriter {
            conn,
            final_path: final_path.to_path_buf(),
            tmp_path,
        })
    }

    pub fn append_file(&mut self, file: &FileRecord) -> Result<()> {
        let mut app = self.conn.appender("files")?;
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

    pub fn append_symbol(&mut self, file_id: FileId, sym: &Symbol) -> Result<()> {
        let mut app = self.conn.appender("symbols")?;
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
        app.flush()?;
        Ok(())
    }

    pub fn append_edge(&mut self, edge: &Edge) -> Result<()> {
        let mut stmt = self.conn.prepare(
            "INSERT INTO edges (src_symbol_id, dst_symbol_id, dst_unresolved, kind, file_id) VALUES (?, ?, ?, ?, ?)",
        )?;
        stmt.execute(params![
            edge.src.as_str(),
            edge.dst.as_ref().map(|d| d.as_str()),
            edge.dst_unresolved.as_deref(),
            edge.kind.as_str(),
            edge.file_id,
        ])?;
        Ok(())
    }

    pub fn append_finding(&mut self, finding: &Finding) -> Result<()> {
        let mut stmt = self.conn.prepare(
            "INSERT INTO findings (rule_id, file_id, start_line, end_line, message) VALUES (?, ?, ?, ?, ?)",
        )?;
        stmt.execute(params![
            finding.rule_id.as_str(),
            finding.file_id,
            finding.start_line as i32,
            finding.end_line as i32,
            finding.message.as_str(),
        ])?;
        Ok(())
    }

    pub fn append_parse_error(&mut self, err: &ParseError) -> Result<()> {
        let mut app = self.conn.appender("parse_errors")?;
        app.append_row(params![
            err.file_id,
            err.message.as_str(),
            err.line as i32,
            err.col as i32,
        ])?;
        app.flush()?;
        Ok(())
    }

    pub fn finalize(self) -> Result<()> {
        self.conn.close().map_err(|(_, e)| MallardError::DuckDb(e))?;
        if self.final_path.exists() {
            std::fs::remove_file(&self.final_path)?;
        }
        std::fs::rename(&self.tmp_path, &self.final_path)?;
        Ok(())
    }
}

fn tmp_path_for(final_path: &Path) -> PathBuf {
    let mut s = final_path.as_os_str().to_owned();
    s.push(".tmp");
    PathBuf::from(s)
}

fn seed_metadata(conn: &Connection, meta: &Metadata) -> Result<()> {
    let mut stmt = conn.prepare("INSERT INTO metadata (key, value) VALUES (?, ?)")?;
    stmt.execute(params!["sha", meta.sha.as_str()])?;
    stmt.execute(params!["indexer_version", meta.indexer_version.as_str()])?;
    if let Some(h) = &meta.rule_set_hash {
        stmt.execute(params!["rule_set_hash", h.as_str()])?;
    }
    stmt.execute(params!["built_at", meta.built_at.as_str()])?;
    stmt.execute(params![
        "language_allow_list",
        meta.language_allow_list.join(",").as_str()
    ])?;
    Ok(())
}

#[allow(dead_code)]
pub fn _kind_strs() -> Vec<&'static str> {
    vec![
        SymbolKind::Function.as_str(),
        EdgeKind::Calls.as_str(),
        FileStatus::Indexed.as_str(),
    ]
}
