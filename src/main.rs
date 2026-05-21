use std::path::PathBuf;
use std::process::ExitCode;
use std::str::FromStr;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use mallard::{
    BuildRequest, Direction, EdgeKind, FindingFilter, SymbolId, build, query,
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
    match args.sub {
        QueryCmd::Symbol { id, index } => {
            let out = query::lookup_symbol(&index, &SymbolId(id))?;
            print(&out)
        }
        QueryCmd::Neighbors {
            id,
            index,
            kind,
            direction,
        } => {
            let kinds = parse_kinds(&kind)?;
            let dir = Direction::from_str(&direction)?;
            let out = query::neighbors(&index, &SymbolId(id), &kinds, dir)?;
            print(&out)
        }
        QueryCmd::Expand {
            id,
            index,
            depth,
            kind,
            direction,
        } => {
            let kinds = parse_kinds(&kind)?;
            let dir = Direction::from_str(&direction)?;
            let out = query::expand(&index, &SymbolId(id), depth, &kinds, dir)?;
            print(&out)
        }
        QueryCmd::Findings {
            index,
            rule,
            path_prefix,
            symbol_id,
        } => {
            let filter = FindingFilter {
                rule_id: rule,
                path_prefix,
                symbol_id: symbol_id.map(SymbolId),
            };
            let out = query::findings(&index, filter)?;
            print(&out)
        }
        QueryCmd::SymbolsInFile { path, index } => {
            let out = query::symbols_in_file(&index, &path)?;
            print(&out)
        }
        QueryCmd::ImportersOf { path, index } => {
            let out = query::importers_of_file(&index, &path)?;
            print(&out)
        }
        QueryCmd::Files { index, prefix } => {
            let out = query::files_at_prefix(&index, &prefix)?;
            print(&out)
        }
        QueryCmd::Metadata { index } => {
            let out = query::metadata(&index)?;
            print(&out)
        }
    }
}
