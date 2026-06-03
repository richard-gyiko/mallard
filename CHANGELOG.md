# Changelog

All notable changes to mallard are documented here. Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versioning follows [SemVer](https://semver.org).

## [Unreleased]

## [0.1.3] — 2026-06-03

### Added

- **Caller-drop / dead-import findings** in `pr-review` (Stage 6). A symbol removed or renamed in the PR whose call sites or imports were not updated now surfaces as a `caller-drop` (still called) or `dead-import` (still imported) finding at tier `structural-rule`, rendered in a "⚠️ Structural breakage" section. This is the marquee cross-SHA signal a live single-state index cannot compute (the Kiro "updated the definition, missed the call sites" failure mode). Reuses `symbol_diff` + `unresolved_callers`; clean renames produce nothing, a rename that misses sites fires.
- **Test-gap detection** (`--flag-test-gaps`, opt-in; also a GitHub Action input). Annotates a `modified-body` comment whose symbol has zero test seams in the index — a modified callable no test exercises. Zero-seam only (no staleness guessing); off by default to stay low-noise on low-coverage repos.

### Changed

- GitHub Action default `mallard-rev` bumped `v0.1.2` → `v0.1.3`.
- Positioning locked as the **cross-SHA verification layer** vs comprehension tools (codegraph / LSP / embeddings) — see [ADR-0014](docs/decisions/0014-verification-layer-vs-comprehension-layer.md). `docs/system.md`, `skills/mallard/SKILL.md`, and the README "How mallard compares" section updated; the stale "No MCP wrapper" note in `docs/plans/workflow-fit-and-contract.md` superseded.

## [0.1.2] — 2026-05-26

### Fixed

- **Release workflow**: install `g++-aarch64-linux-gnu` and persist `CC_aarch64_*` / `CXX_aarch64_*` via `$GITHUB_ENV` so cc-rs picks up the C++ cross-compiler. v0.1.1's release matrix had only the C cross-compiler installed, so bundled DuckDB compilation failed on the `aarch64-unknown-linux-gnu` leg and no GitHub Release was created.

### Changed

- GitHub Action default `mallard-rev` bumped `v0.1.0` → `v0.1.2`. Skips v0.1.1 (no published binaries).

## [0.1.1] — 2026-05-26

Tagged but not fully released — the `aarch64-unknown-linux-gnu` build leg failed (missing C++ cross-compiler). No binary assets on the Releases page for this tag. v0.1.2 supersedes it.

### Changed

- **Enable libduckdb-sys `bundled` feature** on the `duckdb` dependency. v0.1.0's source install (`cargo install --git`) failed on Linux because the system was expected to provide `libduckdb`. Bundled compiles the DuckDB amalgamation as part of the cargo build — works on every platform but adds ~5-10 min to a cold build and ~10 MB to the binary.
- GitHub Action default `mallard-rev` pinned to `v0.1.0` (was unpinned `"main"`).

## [0.1.0] — 2026-05-26

First public release. Mallard ships as a deterministic, citation-grounded code-index for verifying AI-generated code changes.

### Added

- **Four agent-facing CLI primitives** under JSON contract v1.0 (envelope: `schema_version: "1.0"`):
  - `mallard query find --qname X` — qualified-name lookup (exact + dot/colon suffix)
  - `mallard query blast-radius --qname X` — composite `{symbol, callers, callees, test_seams, other_qname_matches}`
  - `mallard query test-seams --qname X` — standalone test-seam discovery
  - `mallard symbol-diff --base-db --head-db` — cross-index added/removed/modified symbols
- **Power-user CLI surface** (unversioned, stable shape per ADR-0007 composition contract): `query symbol`, `neighbors`, `expand`, `findings`, `symbols-in-file`, `edges-by-file`, `unresolved-callers`, `importers-of`, `files`, `metadata`, `pr-review`, `diff-hunks`, `index`.
- **Agent skill manifest** at `skills/mallard/SKILL.md` (Anthropic Agent Skills format). Distributed via [skills.sh](https://www.skills.sh): `npx skills add richard-gyiko/mallard`.
- **GitHub Action** (`.github/actions/review/`) for CI-time PR verification.
- **Pre-built binaries** for Linux (x86_64, ARM64), macOS (x86_64, ARM64), Windows (x86_64) on every tag push.
- **Language support**: Rust, Python, TypeScript, JavaScript.
- **Confidence tier model** per ADR-0010: `structural-rule | extracted | inferred | ambiguous | unresolved`.

### Documented

- [`docs/cli-json-contract.md`](docs/cli-json-contract.md) — locked v1.0 JSON schemas + migration policy.
- [`docs/system.md`](docs/system.md) — architecture overview.
- [`docs/decisions/`](docs/decisions/) — 13 ADRs covering language choice, store, parsing, retrieval strategy, indexing model, wedge, confidence model, deterministic-only commitment, multi-language extractor architecture, and the agent-verification pivot.
- [`docs/research/agent-pr-quality-methodology.md`](docs/research/agent-pr-quality-methodology.md) — research methodology for the upcoming State-of-AI-PRs report.

### Project posture

- **Deterministic only.** No LLM integration, ever — per ADR-0013.
- **License:** MIT.

[Unreleased]: https://github.com/richard-gyiko/mallard/compare/v0.1.2...HEAD
[0.1.2]: https://github.com/richard-gyiko/mallard/releases/tag/v0.1.2
[0.1.1]: https://github.com/richard-gyiko/mallard/releases/tag/v0.1.1
[0.1.0]: https://github.com/richard-gyiko/mallard/releases/tag/v0.1.0
