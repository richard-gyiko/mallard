# Mallard CLI JSON contract v1.0

**Audience:** agent-skill authors, MCP-server wrappers, downstream tooling that parses mallard JSON output.

**Stability promise:** every command listed here emits output that conforms to schema v1.0. Breaking shape changes bump `schema_version` major. Additive fields do not.

## Versioning model

| surface | versioning |
|---|---|
| `mallard query find` / `blast-radius` / `test-seams` | wrapped in `schema_version` envelope (v1.0) |
| `mallard symbol-diff` | wrapped in `schema_version` envelope (v1.0) |
| `mallard pr-review` | unwrapped (ADR-0007 / Action consumer back-compat) |
| `mallard query symbol` / `neighbors` / `expand` / `findings` / `symbols-in-file` / `edges-by-file` / `unresolved-callers` / `importers-of` / `files` / `metadata` | unwrapped (ADR-0007 composition contract) |

Agents reading the versioned surface MUST check `schema_version` before parsing. Non-`"1.0"` values mean breaking changes — fall back or fail loudly.

## Agent-facing primitives (versioned)

### `mallard query find --index DB --qname X`

Find symbols by qualified name. Exact `qualified_name = X` matches rank first; suffix matches (`*.X`) follow.

```jsonc
{
  "schema_version": "1.0",
  "kind": "find_by_qname",
  "value": [
    {
      "id": "e236ff790e58e0335c2def84a4aeb617",
      "file_id": 3,
      "path": "lib.rs",
      "qualified_name": "double",
      "kind": "function",
      "signature": "(x: i64)",
      "anchor": {
        "start_byte": 43, "end_byte": 87,
        "start_line": 4, "end_line": 6
      }
    }
  ]
}
```

Empty array on no match (NOT null). `kind` values: `function | method | macro | class | struct | enum | trait | interface | type_alias | const | static | module | other`.

### `mallard query blast-radius --index DB --qname X`

Composite: qname → top match → callers + callees + test seams. Single-shot for "what breaks if I touch X?"

```jsonc
{
  "schema_version": "1.0",
  "kind": "blast_radius",
  "value": {
    "symbol": { /* SymbolRecord */ },
    "callers": [ /* SymbolRecord, ... */ ],
    "callees": [ /* SymbolRecord, ... */ ],
    "test_seams": [ /* SymbolRecord, ... */ ],
    "other_qname_matches": [ /* SymbolRecord, ... */ ]
  }
}
```

`value: null` if qname matches no symbol. `other_qname_matches` surfaces sibling matches (e.g. shared short names across modules) — agents disambiguate via `path` + `kind`.

`test_seams` is the subset of `callers` whose path or qname is classified as test (see `is_test_symbol` in `src/pr_review.rs` for the rules).

### `mallard query test-seams --index DB --qname X`

Standalone test-seam lookup. Subset of `blast-radius.test_seams`. Lighter when only test-seam info is needed.

```jsonc
{
  "schema_version": "1.0",
  "kind": "test_seams",
  "value": [ /* SymbolRecord, ... */ ]
}
```

Empty array on no match or no test callers.

### `mallard symbol-diff --base-db DB1 --head-db DB2`

Lightweight cross-index symbol diff. Cheaper than `pr-review` — no rule findings, no comment budget. Symbols match by `(qualified_name, path, signature)` tuple.

```jsonc
{
  "schema_version": "1.0",
  "added": [ /* SymbolRecord, ... */ ],
  "removed": [ /* SymbolRecord, ... */ ],
  "modified": [
    {
      "qualified_name": "Counter::bump",
      "path": "lib.rs",
      "base": { /* SymbolRecord */ },
      "head": { /* SymbolRecord */ }
    }
  ]
}
```

`modified` = present in both indexes with same `(qualified_name, path, signature)` but different `anchor` (byte range or line range changed → body edit).

## Power-user primitives (unversioned, v1.0 by convention)

Listed for completeness. Shape locked but NOT carrying `schema_version` field — back-compat with ADR-0007 composition patterns.

| command | returns |
|---|---|
| `query symbol <id>` | `QueryResult::LookupSymbol` — `Option<SymbolRecord>` |
| `query neighbors <id>` | `QueryResult::Neighbors` — `Vec<NeighborEdge>` |
| `query expand <id> --depth N` | `QueryResult::Expand` — `Subgraph` |
| `query findings` | `QueryResult::Findings` — `Vec<FindingRecord>` |
| `query symbols-in-file <path>` | `QueryResult::SymbolsInFile` — `Vec<SymbolRecord>` |
| `query edges-by-file <path>` | `QueryResult::EdgesByFile` — `Vec<FileEdgeBundle>` |
| `query unresolved-callers --name X` | `QueryResult::UnresolvedCallers` — `Vec<UnresolvedCallerHit>` |
| `query importers-of <path>` | `QueryResult::ImportersOfFile` — `Vec<SymbolRecord>` |
| `query files [--prefix X]` | `QueryResult::FilesAtPrefix` — `Vec<FileRecordOut>` |
| `query metadata` | `QueryResult::Metadata` — `MetadataRecord` |
| `pr-review --base-db --head-db` | `pr_review::PrReviewResult` — `findings + summary` (consumed by GitHub Action) |
| `diff-hunks --base-sha --head-sha` | `pr_review::DiffHunks` — per-file ranges |
| `index --sha X` | `BuildSummary` — counters + timing |

Rust source-of-truth: types defined in `src/query.rs` and `src/pr_review.rs`, exported from `mallard::*` in `src/lib.rs`.

## Shared types

### `SymbolRecord`

```jsonc
{
  "id": "32-hex-char content hash",
  "file_id": 3,
  "path": "lib.rs",
  "qualified_name": "module::path::Symbol",
  "kind": "function" /* see kinds above */,
  "signature": "(x: i64) -> u32",
  "anchor": {
    "start_byte": 43, "end_byte": 87,
    "start_line": 4, "end_line": 6
  }
}
```

### `Anchor`

Byte and line span of the symbol's definition in its file. 1-indexed lines. Byte offsets are UTF-8 byte positions.

### Confidence tiers (where present)

- `structural-rule` — derivable from grammar / declarations alone
- `extracted` — read directly from source token
- `inferred` — heuristic resolution (e.g. unqualified name → most likely binding)
- `ambiguous` — multiple candidate resolutions; result is best-effort
- `unresolved` — name not found in index (often stdlib / external)

## Exit codes

| code | meaning |
|---|---|
| 0 | success — JSON on stdout |
| 1 | runtime error — message on stderr, no JSON |
| 2 | argument parse error (clap) — usage on stderr |
| 127 | binary not on PATH (standard shell convention) |

Agent skill bodies SHOULD check exit code 0 before parsing stdout.

## Migration policy

- **Additive fields** (new keys on existing structs): no version bump. Skill consumers MUST tolerate unknown keys.
- **Removed fields or renamed fields**: schema_version major bump (v1.0 → v2.0). Old binary keeps emitting v1.0 until users opt-in via flag.
- **Renamed commands**: covered by major bump.
- **Unversioned primitives**: covered by SemVer of the `mallard` binary itself. Major-version bump means breaking shape changes. ADR-0007 consumers pin to a minor.

## Cross-references

- [`src/query.rs`](../src/query.rs) — type definitions
- [`src/pr_review.rs`](../src/pr_review.rs) — pr-review output type + test-symbol classification
- [`docs/decisions/0007-defer-retrieval-module-agents-compose-primitives.md`](decisions/0007-defer-retrieval-module-agents-compose-primitives.md) — composition contract for power-user surface
- [`docs/plans/workflow-fit-and-contract.md`](plans/workflow-fit-and-contract.md) — agent workflow mapping
- `SKILL.md` (root, after week 1-2) — agent invocation contract
