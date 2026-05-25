# Contributing to mallard

Thanks for your interest. Mallard is a deterministic, citation-grounded code-index for verifying AI-generated code changes. Contributions are welcome — bug reports, language extractors, structural rules, docs, and skill recipes.

## Before you start

- Read [`CLAUDE.md`](CLAUDE.md) for project-wide instructions on documentation lookups.
- Read [`CONTEXT.md`](CONTEXT.md) for the domain language (`Index`, `IndexReader`, `Symbol`, `Edge`, `ParsedSource`).
- Read [`docs/system.md`](docs/system.md) for architecture.
- Read [`docs/decisions/`](docs/decisions/) for design constraints. The most load-bearing ADRs:
  - [ADR-0007](docs/decisions/0007-defer-retrieval-module-agents-compose-primitives.md) — agents compose primitives via CLI
  - [ADR-0009](docs/decisions/0009-pr-review-architecture-pattern.md) — layered pipeline
  - [ADR-0010](docs/decisions/0010-edge-confidence-tier.md) — confidence tier model
  - [ADR-0013](docs/decisions/0013-kill-phase-d-pivot-agent-verification.md) — deterministic-only is permanent

## Out of scope — do NOT submit

- **LLM integration of any form** in the core library. Mallard ships zero LLM calls forever per ADR-0013. PRs that add LLM dependencies will be closed.
- **SAST / security-pattern rules.** Semgrep / Snyk / CodeQL own that lane.
- **Auto-fix / refactor suggestions.** Mallard is read-only by design.
- **Per-developer attribution / blame.** Toxic vector, not in scope.

## Development setup

Requires Rust stable (edition 2024) and a C/C++ toolchain for `tree-sitter` and `duckdb`.

```bash
git clone https://github.com/richard-gyiko/mallard
cd mallard
cargo build --release
cargo test --release
```

On Windows, build under PowerShell or `cmd` — git-bash currently fails the release binary with `STATUS_DLL_NOT_FOUND`.

## Pull request checklist

Every PR must pass CI:

- `cargo build --release --locked`
- `cargo test --release --locked`
- `cargo fmt --check`
- `cargo clippy --release --all-targets -- -D warnings`

Beyond that:

- **Cite the contract.** If your change touches a public CLI surface or JSON schema, update [`docs/cli-json-contract.md`](docs/cli-json-contract.md) and bump `schema_version` if breaking.
- **Add tests.** Integration tests live in `tests/`. New language extractors need fixtures under `tests/fixtures/`.
- **Update CONTEXT.md** if you introduce a new domain term.
- **Add an ADR** for non-trivial design choices. ADRs are append-only; supersede via a new ADR rather than editing the old one.
- **No unnecessary comments.** Default to no comments. Add only when the WHY is non-obvious.
- **No backwards-compat shims** in pre-1.0 releases. Change the code directly.
- **Keep commits clean.** PRs land via squash-merge by default, but well-structured branch commits help review.

## Reporting bugs

Open an issue with:

- mallard version (`mallard --version`)
- OS + Rust toolchain
- Minimal reproducer (a small repo + the failing command + JSON output)
- Expected vs actual behavior

Bonus: a `mallard index` of the failing repo attached as a `.duckdb` file makes triage instant.

## Adding a language extractor

Mallard's `SymbolExtractor` seam (`src/extractor.rs`) takes a tree-sitter grammar and an extraction strategy. The path:

1. Add the tree-sitter crate to `Cargo.toml`.
2. Implement `SymbolExtractor` for the language (see `src/extractor_python.rs` / `src/extractor_typescript.rs` as references).
3. Wire it into `file_processor.rs`.
4. Add fixtures under `tests/fixtures/sample-<lang>/`.
5. Add an integration test in `tests/index_build_integration.rs` + `tests/index_query_integration.rs`.
6. Update `CONTEXT.md` and the language list in `README.md`.

ADR-0012 describes the multi-language extractor architecture.

## Adding a structural rule

Rules live in YAML (`assets/rules-default.yml` for the bundled pack, or per-repo `.mallard/rules.yml`). See `src/rules.rs` for the schema and existing rules for patterns. PRs adding rules to the bundled pack must include:

- Real-world example matches (link to PRs that would have caught the issue)
- A confidence tier classification per ADR-0010
- Test coverage in `tests/fixtures/`

## Communication

- **Bug reports / feature requests:** GitHub Issues
- **Strategic / design discussion:** GitHub Discussions (when enabled) or open an issue tagged `design`
- **Security:** see [`SECURITY.md`](SECURITY.md)

## License

By contributing, you agree your contributions are licensed under MIT (see [`LICENSE`](LICENSE)).
