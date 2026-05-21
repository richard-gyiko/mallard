---
name: mallard
description: Query a mallard repository index from the shell to anchor reasoning to structural facts (symbol lookups, callers/callees, bounded neighborhood expansion, structural rule findings, file/module queries). Use when a task needs to know what a function does, who calls it, what it likely breaks, or what structural rules a file violates — especially for PR-review-shaped work (diff blast radius, evidence-grounded review comments). Composes with `jq`, `grep`, `gh`, `git` in the same Bash chain. Skip when the task is fuzzy / natural-language search across the codebase — mallard is symbolic-first and has no embedding fallback.
---

# mallard

Background: [CONTEXT.md](../../../CONTEXT.md) defines domain terms (`Index`, `IndexReader`, `Symbol`, `Edge`, `ParsedSource`). [docs/specs/indexing/index-query.md](../../../docs/specs/indexing/index-query.md) is the read-primitive contract. [ADR-0007](../../../docs/decisions/0007-defer-retrieval-module-agents-compose-primitives.md) is why retrieval is agent-composed via this CLI rather than a built module.

## Prereqs

- `cargo` (to build mallard) or a prebuilt `mallard` binary on `$PATH`.
- `jq` for JSON extraction in Bash, or PowerShell's `ConvertFrom-Json` instead.
- `gh` only if pulling PR metadata (optional).

## Running mallard

Two equivalent ways to invoke; pick one and stick with it for a session.

**Via cargo (development)**:
```bash
cargo run --quiet -- query <verb> ... 2>/dev/null
```
Always redirect stderr (`2>/dev/null` in bash, `2>$null` in PowerShell). Cargo writes incremental-compilation warnings to stderr that will corrupt JSON capture if you don't.

**Via prebuilt binary (recommended for piping / scripts)**:
```bash
cargo build --release
./target/release/mallard query <verb> ...
```
Clean stdout, no cargo noise, faster repeat calls. On Windows, run from PowerShell or cmd, not git-bash — the release binary fails under MSYS2's DLL loader (`STATUS_DLL_NOT_FOUND`).

## Invariants

- Each `mallard query <verb>` opens the index, verifies `index_format_version`, runs one read, exits. No persistent server.
- Stdout is JSON: `{"kind": "<verb-name>", "value": <payload>}`. Stderr is tracing + errors. Exit 0 on success, 1 on error.
- `--index <path>` is required on every `query` subcommand.
- Indexes are immutable per SHA. Two SHAs = two indexes; never edit in place.
- Cross-file calls resolve heuristically per [ADR-0008](../../../docs/decisions/0008-heuristic-name-resolution.md). Stdlib / external crate calls stay `dst_unresolved` by design.
- Every edge carries `confidence: extracted | inferred | ambiguous | unresolved` per [ADR-0010](../../../docs/decisions/0010-edge-confidence-tier.md). `ambiguous` surfaces previously-silent multi-match drops — high-priority for human disambiguation. `extracted` = intra-file resolved; `inferred` = post-build cross-file resolved; `unresolved` = no candidate found.

## Indexing

```bash
mallard index <repo-path> --sha <commit-sha> [--out <path>] [--lang rust] [--rules <yaml>] [--max-file-bytes N]
```

- `--sha` is the commit SHA. Free-form strings work for ad-hoc runs (`--sha dogfood`); use real SHAs for PR-review work.
- `--out` defaults to `./.mallard/index-<sha-prefix>.duckdb`.
- `--lang` repeats for an allow-list. Today only Rust is supported; omit to detect.
- `--rules <yaml>` enables the structural-rules engine. Without it, `findings` is empty.

Build is ephemeral per [ADR-0005](../../../docs/decisions/0005-ephemeral-indexing-defer-incremental.md) — each invocation rebuilds from scratch.

## Query verbs

All verbs require `--index <path>`. Examples assume `INDEX=./.mallard/index-dogfood.duckdb`.

### `metadata`

```bash
mallard query metadata --index "$INDEX"
```

Returns `{sha, indexer_version, rule_set_hash, built_at, language_allow_list, index_format_version}`. Confirms an index file exists, matches the expected SHA, and was built with the expected rules.

### `symbol <id>`

Point lookup. Returns `null` if absent. Derive IDs via `symbols-in-file` first; don't construct by hand.

```bash
mallard query symbol <id> --index "$INDEX"
```

### `symbols-in-file <path>`

All symbols defined in a file, ordered by anchor start byte. Most common entry point.

```bash
mallard query symbols-in-file src/query.rs --index "$INDEX"
```

### `neighbors <id> --kind <k1,k2> --direction <in|out|both>`

Direct neighbors along the requested edge kinds. `--kind` accepts `calls`, `imports`, `contains`, `tests_for`, `tested_by`; omit for all kinds.

```bash
mallard query neighbors <id> --kind calls --direction in --index "$INDEX"   # callers
mallard query neighbors <id> --kind calls --direction out --index "$INDEX"  # callees
```

### `expand <id> --depth N --kind ... --direction ...`

Bounded BFS. Returns `{nodes, edges, max_depth_reached}`. `--depth 0` returns the source alone. Cycles broken by a visited set — arbitrary depths terminate when the frontier empties.

```bash
mallard query expand <id> --depth 2 --kind calls --direction out --index "$INDEX"
```

### `findings [--rule <id>] [--path-prefix <p>] [--symbol-id <id>]`

Structural rule findings. All filters optional and combinable. `--symbol-id` scopes to findings whose line range overlaps the symbol's anchor.

```bash
mallard query findings --rule rust-format-macro --index "$INDEX"
mallard query findings --path-prefix src/ --index "$INDEX"
mallard query findings --symbol-id <id> --index "$INDEX"
```

### `edges-by-file <path> --kind <k1,k2> --direction <in|out|both>`

Bulk per-file edges: every symbol in the file, plus its `outbound` and `inbound` edges (peer-enriched). Symbols with no edges still appear with empty arrays so callers can set-diff bundles across two indexes without re-querying.

```bash
mallard query edges-by-file src/query.rs --kind calls --direction both --index "$INDEX"
```

One SQL query per active direction, collapsing the N+1 pattern of looping `neighbors` over `symbols-in-file`. Use for per-file blast-radius analysis and PR-review stage-4 edge diff.

### `unresolved-callers --name n1,n2,... --kind k1,k2`

All call sites pointing at any of the given unresolved names. One SQL query against the edges table. Use for orphan-caller scans (e.g. after removing a public function — every site that didn't migrate to the replacement shows up here).

```bash
mallard query unresolved-callers --name format,unwrap --kind calls --index "$INDEX"
```

### `files [--prefix <p>]`

File records (path, language, size, status). Empty prefix = all files.

```bash
mallard query files --prefix src/ --index "$INDEX"
```

### `importers-of <path>`

Symbols whose file imports the given file path. Sparse today — see Gotchas.

```bash
mallard query importers-of src/parsed_source.rs --index "$INDEX"
```

## Composition with jq

Every output is `{"kind", "value"}`. Pipe through `jq`:

```bash
mallard query metadata --index "$INDEX" | jq -r '.value.sha'
mallard query symbols-in-file src/query.rs --index "$INDEX" | jq '.value[] | {q: .qualified_name, k: .kind, id: .id}'
mallard query neighbors <id> --kind calls --direction in --index "$INDEX" | jq '.value[].src.qualified_name'
```

### Find symbol → expand neighborhood

```bash
ID=$(mallard query symbols-in-file src/query.rs --index "$INDEX" \
     | jq -r '.value[] | select(.qualified_name == "IndexReader::run") | .id')
mallard query expand "$ID" --depth 2 --kind calls --direction out --index "$INDEX"
```

### Scope findings to one symbol

```bash
ID=$(mallard query symbols-in-file src/extractor.rs --index "$INDEX" \
     | jq -r '.value[] | select(.qualified_name == "RustExtractor::extract") | .id')
mallard query findings --symbol-id "$ID" --index "$INDEX"
```

### List unresolved callees (likely stdlib / external)

```bash
mallard query neighbors "$ID" --kind calls --direction out --index "$INDEX" \
  | jq -r '.value[] | select(.dst == null) | .dst_unresolved' \
  | sort -u
```

### PR-review-shaped recipes

Multi-step recipes for diff blast radius and base/head PR-review chains live in [references/pr-review-recipes.md](references/pr-review-recipes.md). Read that file when assembling evidence for an LLM reviewer over a real diff.

## Gotchas

- **Cross-file resolution is heuristic.** ~10% of cross-file calls resolve on a typical Rust repo; the rest are stdlib / third-party. `dst_unresolved` for those is correct, not a bug.
- **`importers_of_file` is currently sparse.** Imports edges carry the whole `use_declaration` text in `dst_unresolved` rather than per-symbol targets. Until the parser splits imports per path, this query returns mostly empty results. Use `neighbors --kind calls --direction in` against a specific symbol instead when asking "who depends on X".
- **Constructor calls filtered out.** Rust tuple-struct / enum-variant constructors (`Ok(x)`, `Some(x)`, `SymbolId(s)`, scoped `QueryRequest::LookupSymbol(x)`) do **not** appear as `calls` edges per [ADR-0008](../../../docs/decisions/0008-heuristic-name-resolution.md). Don't hunt for `Ok` as a callee.
- **Same qualified name in two files** → two distinct symbol IDs (file path is part of the hash). The resolver picks the unambiguous callable; ambiguous matches stay `dst_unresolved`.
- **`findings` is empty without `--rules`** at index time. Rebuild with `--rules <yaml>` if you need them.
- **Const / static / type-alias symbols carry no `calls` edges.** They aren't *called*, only *referenced* — and references aren't extracted today (per [ADR-0008](../../../docs/decisions/0008-heuristic-name-resolution.md)). Absence of edges on these kinds means "this is a non-callable definition", not "this is dead code".
- **Git Bash on Windows mangles absolute paths.** `--index /foo/bar.duckdb` gets rewritten. Use relative paths or PowerShell.
