---
name: mallard
description: Query a mallard repository index from the shell. Use whenever the task needs structural facts about a codebase that mallard has indexed (or could index) — symbol lookups, callers/callees, bounded neighborhood expansion, structural rule findings, file/module queries. Especially useful for PR-review-shaped tasks (what does this diff touch, who calls it, what does it likely break). Composes with `jq`, `grep`, `gh`, and the rest of your Bash toolbox.
---

# mallard

Repository index over a DuckDB file, queryable from the shell via `mallard query <verb>`. Used to anchor LLM reasoning to structural facts (symbol IDs, edges, anchors, findings) rather than guessing.

See:
- [CONTEXT.md](../../../CONTEXT.md) — domain language (`Index`, `IndexReader`, `Symbol`, `Edge`, `ParsedSource`, etc.).
- [docs/specs/indexing/index-query.md](../../../docs/specs/indexing/index-query.md) — read primitive contract.
- [ADR-0007](../../../docs/decisions/0007-defer-retrieval-module-agents-compose-primitives.md) — why retrieval is agent-composed, not a built module.

## Invariants

- Every `mallard query <verb>` opens the index, verifies `index_format_version`, runs one read, exits. No persistent server.
- Stdout = JSON enveloped as `{"kind": "<verb-name>", "value": <payload>}`. Stderr = tracing + errors. Exit 0 on success, 1 on error.
- `--index <path>` is required on every `query` subcommand. Defaults to none — the caller picks.
- An index is immutable for the SHA it was built from. Two SHAs = two indexes; no in-place updates.
- Cross-file calls resolve via a heuristic (intra-file map + unambiguous-callable post-build pass) per [ADR-0008](../../../docs/decisions/0008-heuristic-name-resolution.md). Stdlib / external crate calls stay `dst_unresolved`. Don't treat the unresolved tail as a parser bug — it's the boundary.

## Indexing

```bash
mallard index <repo-path> --sha <commit-sha> [--out <path>] [--lang rust] [--rules <yaml>] [--max-file-bytes N]
```

- `--sha` is the commit SHA the index represents. Free-form string allowed for ad-hoc indexing (`--sha dogfood`); use the real commit SHA for PR-review-shaped work.
- `--out` defaults to `./.mallard/index-<sha-prefix>.duckdb`.
- `--lang` repeats for an allow-list (`--lang rust --lang python`). Empty = all detectable.
- `--rules <yaml>` enables the structural-rules engine. Without it, `findings` is empty.

Build is ephemeral per [ADR-0005](../../../docs/decisions/0005-ephemeral-indexing-defer-incremental.md). Each invocation rebuilds from scratch.

## Query verbs

All verbs require `--index <path>`. Examples below assume `INDEX=./.mallard/index-dogfood.duckdb`.

### `metadata`

```bash
mallard query metadata --index "$INDEX"
```

Returns `{sha, indexer_version, rule_set_hash, built_at, language_allow_list, index_format_version}`. Use this to confirm an index file exists, matches the SHA you expect, and was built with the rules you expect.

### `symbol <id>`

Point lookup by `SymbolId` (32-char content-addressed hash).

```bash
mallard query symbol <id> --index "$INDEX"
```

Returns `null` if absent. Don't construct symbol IDs by hand — derive them via `symbols-in-file` first.

### `symbols-in-file <path>`

All symbols defined in a file, ordered by anchor start byte.

```bash
mallard query symbols-in-file src/query.rs --index "$INDEX"
```

This is the most common entry point — once you have one symbol's `id`, you can hop the graph.

### `neighbors <id> --kind <k1,k2> --direction <in|out|both>`

Direct neighbors of a symbol along the requested edge kinds.

```bash
mallard query neighbors <id> --kind calls --direction in --index "$INDEX"   # callers
mallard query neighbors <id> --kind calls --direction out --index "$INDEX"  # callees
mallard query neighbors <id> --kind imports --direction in --index "$INDEX" # importers (currently sparse — see Gotchas)
```

`--kind` accepts `calls`, `imports`, `contains`, `tests_for`, `tested_by`. Empty / omitted = all kinds.

### `expand <id> --depth N --kind ... --direction ...`

Bounded BFS from a symbol. Returns a `{nodes, edges, max_depth_reached}` subgraph.

```bash
mallard query expand <id> --depth 2 --kind calls --direction out --index "$INDEX"
```

`--depth 0` returns the source symbol alone. Cycles broken by a visited set; you can pass arbitrarily large depth without explosion (terminates when the frontier empties).

### `findings [--rule <id>] [--path-prefix <p>] [--symbol-id <id>]`

Structural rule findings. All filters optional; combinable.

```bash
mallard query findings --rule rust-format-macro --index "$INDEX"
mallard query findings --path-prefix src/ --index "$INDEX"
mallard query findings --symbol-id <id> --index "$INDEX"   # findings inside this symbol's anchor range
```

### `files [--prefix <p>]`

File records (path, language, size, status). `--prefix` filters by path prefix; empty = all files.

```bash
mallard query files --prefix src/ --index "$INDEX"
```

### `importers-of <path>`

Symbols whose file imports the given file path. Sparse today — see Gotchas.

```bash
mallard query importers-of src/parsed_source.rs --index "$INDEX"
```

## JSON envelope

Every query prints `{"kind": "<verb>", "value": <payload>}`. Pipe through `jq` for extraction:

```bash
mallard query metadata --index "$INDEX" | jq -r '.value.sha'
mallard query symbols-in-file src/query.rs --index "$INDEX" | jq '.value[] | {q: .qualified_name, k: .kind, id: .id}'
mallard query neighbors <id> --kind calls --direction in --index "$INDEX" | jq '.value[].src.qualified_name'
```

## Composition patterns

The whole point of being a CLI: chain with shell.

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

### Walk a diff's blast radius

Given a list of changed file paths from `gh pr diff` or `git diff --name-only`:

```bash
git diff --name-only main HEAD | while read -r f; do
  mallard query symbols-in-file "$f" --index "$INDEX" \
    | jq -r '.value[].id' \
    | while read -r id; do
        mallard query neighbors "$id" --kind calls --direction in --index "$INDEX"
      done
done | jq -s 'add | .value // .'
```

### PR-review-shaped query (build → expand around changed symbols → cite findings)

See [ADR-0009](../../../docs/decisions/0009-pr-review-architecture-pattern.md) for the layered pipeline this composes into. Sketch:

```bash
# 1. Build indexes for base + head (CI / cron).
mallard index . --sha "$BASE_SHA" --out base.duckdb --rules rules.yml
mallard index . --sha "$HEAD_SHA" --out head.duckdb --rules rules.yml

# 2. For each changed file, list head symbols not in base (added/modified).
git diff --name-only "$BASE_SHA" "$HEAD_SHA" | while read -r f; do
  comm -23 \
    <(mallard query symbols-in-file "$f" --index head.duckdb | jq -r '.value[].id' | sort) \
    <(mallard query symbols-in-file "$f" --index base.duckdb | jq -r '.value[].id' | sort)
done

# 3. Expand each changed symbol's blast radius via head index, attach findings.
# 4. Hand the resulting evidence to the LLM for synthesis.
```

## Gotchas

- **Cross-file resolution is heuristic.** Only ~10% of cross-file calls resolve on a typical Rust repo; the rest are stdlib / third-party. `dst_unresolved` for those is correct, not a bug.
- **`importers_of_file` is currently sparse.** Imports edges carry the whole `use_declaration` text in `dst_unresolved` rather than per-symbol targets. Until the parser splits imports per path, this query returns mostly empty results. Use `neighbors --kind calls --direction in` against a specific symbol instead when you want "who depends on X".
- **Constructor calls filtered out.** Rust tuple-struct / enum-variant constructors (`Ok(x)`, `Some(x)`, `SymbolId(s)`, scoped `QueryRequest::LookupSymbol(x)`) do **not** appear as `calls` edges per [ADR-0008](../../../docs/decisions/0008-heuristic-name-resolution.md). Don't go hunting for `Ok` as a callee.
- **PascalCase functions** in non-Rust files may be filtered out by the Rust extractor's PascalCase heuristic. Currently we only index Rust, so n/a.
- **Same qualified name in two files** → two distinct symbol IDs (the file path is part of the ID hash). The resolver picks the unambiguous callable; ambiguous matches stay `dst_unresolved`.
- **`findings` is empty without `--rules`** at index time. Rebuild the index with `--rules <yaml>` if you need them.
- **Bash on Windows mangles absolute paths.** `--index /foo/bar.duckdb` gets rewritten by Git Bash. Use relative paths or PowerShell.

## When NOT to use this

- The task is a stylistic / formatting one (use a formatter).
- You want to read a file's source. Mallard tracks *structure*, not source; use `Read` directly.
- You want recent commit activity. Use `git log` / `gh`.
- You want fuzzy / natural-language search across the codebase. Mallard is symbolic-first by ADR-0004; there is no embedding fallback in v0.
