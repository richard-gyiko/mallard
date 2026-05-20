pub const INDEX_FORMAT_VERSION: u32 = 1;

pub const DDL: &str = r#"
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

pub mod tables {
    pub const FILES: &str = "files";
    pub const SYMBOLS: &str = "symbols";
    pub const EDGES: &str = "edges";
    pub const PARSE_ERRORS: &str = "parse_errors";
    pub const FINDINGS: &str = "findings";
    pub const METADATA: &str = "metadata";
}

pub mod cols {
    pub mod files {
        pub const FILE_ID: &str = "file_id";
        pub const PATH: &str = "path";
        pub const LANGUAGE: &str = "language";
        pub const SIZE_BYTES: &str = "size_bytes";
        pub const STATUS: &str = "status";
    }

    pub mod symbols {
        pub const SYMBOL_ID: &str = "symbol_id";
        pub const FILE_ID: &str = "file_id";
        pub const QUALIFIED_NAME: &str = "qualified_name";
        pub const KIND: &str = "kind";
        pub const SIGNATURE: &str = "signature";
        pub const ANCHOR_START_BYTE: &str = "anchor_start_byte";
        pub const ANCHOR_END_BYTE: &str = "anchor_end_byte";
        pub const ANCHOR_START_LINE: &str = "anchor_start_line";
        pub const ANCHOR_END_LINE: &str = "anchor_end_line";
    }

    pub mod edges {
        pub const SRC_SYMBOL_ID: &str = "src_symbol_id";
        pub const DST_SYMBOL_ID: &str = "dst_symbol_id";
        pub const DST_UNRESOLVED: &str = "dst_unresolved";
        pub const KIND: &str = "kind";
        pub const FILE_ID: &str = "file_id";
    }

    pub mod parse_errors {
        pub const FILE_ID: &str = "file_id";
        pub const MESSAGE: &str = "message";
        pub const LINE: &str = "line";
        pub const COL: &str = "col";
    }

    pub mod findings {
        pub const RULE_ID: &str = "rule_id";
        pub const FILE_ID: &str = "file_id";
        pub const START_LINE: &str = "start_line";
        pub const END_LINE: &str = "end_line";
        pub const MESSAGE: &str = "message";
    }

    pub mod metadata {
        pub const KEY: &str = "key";
        pub const VALUE: &str = "value";
    }
}

pub mod metadata_keys {
    pub const SHA: &str = "sha";
    pub const INDEXER_VERSION: &str = "indexer_version";
    pub const RULE_SET_HASH: &str = "rule_set_hash";
    pub const BUILT_AT: &str = "built_at";
    pub const LANGUAGE_ALLOW_LIST: &str = "language_allow_list";
    pub const INDEX_FORMAT_VERSION: &str = "index_format_version";
}
