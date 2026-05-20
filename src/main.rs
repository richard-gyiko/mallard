use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use mallard::{BuildRequest, build};

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
}

#[derive(Parser, Debug)]
struct IndexArgs {
    /// Repository root to index.
    path: PathBuf,

    /// Commit SHA the index represents (caller-supplied in v0).
    #[arg(long)]
    sha: String,

    /// Optional rules YAML.
    #[arg(long)]
    rules: Option<PathBuf>,

    /// Output DuckDB path. Defaults to ./.mallard/index-<sha-prefix>.duckdb.
    #[arg(long)]
    out: Option<PathBuf>,

    /// Maximum file size to parse, in bytes.
    #[arg(long, default_value_t = 1024 * 1024)]
    max_file_bytes: u64,

    /// Language allow-list. Repeat for multiple. Defaults to all supported.
    #[arg(long = "lang")]
    languages: Vec<String>,

    /// Number of slowest-file timings to keep in the summary.
    #[arg(long, default_value_t = 10)]
    slowest_files_n: usize,
}

fn main() -> ExitCode {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    match cli.command {
        Cmd::Index(args) => match run_index(args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        },
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
    let json = serde_json::to_string_pretty(&summary)?;
    println!("{json}");
    Ok(())
}
