use std::path::PathBuf;
use std::process::ExitCode;
use std::str::FromStr;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use mallard::{
    BuildRequest, Direction, EdgeKind, FindingFilter, IndexReader, QueryRequest, SymbolId, build,
};

#[derive(Parser, Debug)]
#[command(name = "mallard", version, about = "AI-native repository index")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Build an index of a repository at a specific commit SHA.
    Index(IndexArgs),
    /// Query a built index.
    Query(QueryArgs),
}

#[derive(Parser, Debug)]
struct IndexArgs {
    path: PathBuf,
    #[arg(long)]
    sha: String,
    #[arg(long)]
    rules: Option<PathBuf>,
    #[arg(long)]
    out: Option<PathBuf>,
    #[arg(long, default_value_t = 1024 * 1024)]
    max_file_bytes: u64,
    #[arg(long = "lang")]
    languages: Vec<String>,
    #[arg(long, default_value_t = 10)]
    slowest_files_n: usize,
}

#[derive(Parser, Debug)]
struct QueryArgs {
    #[command(subcommand)]
    sub: QueryCmd,
}

#[derive(Subcommand, Debug)]
enum QueryCmd {
    /// Point lookup by symbol ID.
    Symbol {
        id: String,
        #[arg(long)]
        index: PathBuf,
    },
    /// Direct neighbors of a symbol.
    Neighbors {
        id: String,
        #[arg(long)]
        index: PathBuf,
        /// Comma-separated edge kinds. Empty = all kinds.
        #[arg(long, default_value = "")]
        kind: String,
        #[arg(long, default_value = "both")]
        direction: String,
    },
    /// Bounded neighborhood expansion.
    Expand {
        id: String,
        #[arg(long)]
        index: PathBuf,
        #[arg(long)]
        depth: u32,
        #[arg(long, default_value = "")]
        kind: String,
        #[arg(long, default_value = "both")]
        direction: String,
    },
    /// Structural rule findings.
    Findings {
        #[arg(long)]
        index: PathBuf,
        #[arg(long)]
        rule: Option<String>,
        #[arg(long = "path-prefix")]
        path_prefix: Option<String>,
        #[arg(long = "symbol-id")]
        symbol_id: Option<String>,
    },
    /// Symbols defined in a file.
    SymbolsInFile {
        path: String,
        #[arg(long)]
        index: PathBuf,
    },
    /// All symbols in a file with their outbound + inbound edges.
    EdgesByFile {
        path: String,
        #[arg(long)]
        index: PathBuf,
        #[arg(long, default_value = "")]
        kind: String,
        #[arg(long, default_value = "both")]
        direction: String,
    },
    /// All call sites pointing at any of the given unresolved names.
    /// Use for orphan-caller scans (e.g. after removing a public function).
    UnresolvedCallers {
        #[arg(long)]
        index: PathBuf,
        /// Comma-separated unresolved names to match.
        #[arg(long, value_delimiter = ',')]
        name: Vec<String>,
        #[arg(long, default_value = "")]
        kind: String,
    },
    /// Symbols whose file imports the given file path.
    ImportersOf {
        path: String,
        #[arg(long)]
        index: PathBuf,
    },
    /// Files whose path starts with the given prefix.
    Files {
        #[arg(long)]
        index: PathBuf,
        #[arg(long, default_value = "")]
        prefix: String,
    },
    /// Index metadata.
    Metadata {
        #[arg(long)]
        index: PathBuf,
    },
}

fn main() -> ExitCode {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    let result = match cli.command {
        Cmd::Index(args) => run_index(args),
        Cmd::Query(args) => run_query(args),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_index(args: IndexArgs) -> anyhow::Result<()> {
    let sha_prefix: String = args.sha.chars().take(12).collect();
    let out = args
        .out
        .unwrap_or_else(|| PathBuf::from(".mallard").join(format!("index-{sha_prefix}.duckdb")));

    let req = BuildRequest {
        root: args.path,
        sha: args.sha,
        rules_path: args.rules,
        out_path: out,
        max_file_bytes: args.max_file_bytes,
        language_allow_list: args.languages,
        slowest_files_n: args.slowest_files_n,
    };

    let summary = build(req)?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

fn parse_kinds(s: &str) -> anyhow::Result<Vec<EdgeKind>> {
    s.split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| EdgeKind::from_str(t).map_err(|_| anyhow::anyhow!("unknown edge kind: {t}")))
        .collect()
}


fn print<T: serde::Serialize>(value: &T) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn run_query(args: QueryArgs) -> anyhow::Result<()> {
    let (index, request) = to_request(args.sub)?;
    let result = IndexReader::open(&index)?.run(&request)?;
    print(&result)
}

fn to_request(cmd: QueryCmd) -> anyhow::Result<(std::path::PathBuf, QueryRequest)> {
    Ok(match cmd {
        QueryCmd::Symbol { id, index } => (index, QueryRequest::LookupSymbol { id: SymbolId(id) }),
        QueryCmd::Neighbors {
            id,
            index,
            kind,
            direction,
        } => (
            index,
            QueryRequest::Neighbors {
                id: SymbolId(id),
                kinds: parse_kinds(&kind)?,
                direction: Direction::from_str(&direction)?,
            },
        ),
        QueryCmd::Expand {
            id,
            index,
            depth,
            kind,
            direction,
        } => (
            index,
            QueryRequest::Expand {
                id: SymbolId(id),
                depth,
                kinds: parse_kinds(&kind)?,
                direction: Direction::from_str(&direction)?,
            },
        ),
        QueryCmd::Findings {
            index,
            rule,
            path_prefix,
            symbol_id,
        } => (
            index,
            QueryRequest::Findings {
                filter: FindingFilter {
                    rule_id: rule,
                    path_prefix,
                    symbol_id: symbol_id.map(SymbolId),
                },
            },
        ),
        QueryCmd::SymbolsInFile { path, index } => (index, QueryRequest::SymbolsInFile { path }),
        QueryCmd::EdgesByFile {
            path,
            index,
            kind,
            direction,
        } => (
            index,
            QueryRequest::EdgesByFile {
                path,
                kinds: parse_kinds(&kind)?,
                direction: Direction::from_str(&direction)?,
            },
        ),
        QueryCmd::UnresolvedCallers { index, name, kind } => (
            index,
            QueryRequest::UnresolvedCallers {
                names: name,
                kinds: parse_kinds(&kind)?,
            },
        ),
        QueryCmd::ImportersOf { path, index } => (index, QueryRequest::ImportersOfFile { path }),
        QueryCmd::Files { index, prefix } => (index, QueryRequest::FilesAtPrefix { prefix }),
        QueryCmd::Metadata { index } => (index, QueryRequest::Metadata),
    })
}
