# Finding 7 — single-line guard inserts inside existing methods stay invisible

## Surfaced by

`mallard-eval` pilot v2 (`mallard-eval/results/pilot-v2.md`). axios PR #10901 (`fix: guard socketPath with own() to prevent prototype pollution SSRF`) produced **zero** comments despite being exactly the class of change mallard should catch — a security-relevant guard inserted into an existing method.

## What goes wrong

mallard's stage 4 has two signals today:

1. **Callee-set delta** — fires when the symbol's outbound calls change between base and head.
2. **Anchor byte-span delta** — fires when the symbol exists at both SHAs with identical callee set but the symbol's `(end_byte - start_byte)` differs (Finding 4 fix, shipped in PR #33).

axios #10901 inserts an `Object.hasOwn(options, "socketPath")` guard. Both signals miss it because:

- The `Object.hasOwn` call **does** add a new callee, but the constructor-filter or PascalCase / TitleCase heuristic drops `Object.hasOwn` before it lands in the edges table (TBD — needs verification on the run JSON).
- OR `Object.hasOwn` reaches the edges table but the resolver tiers it `unresolved` (stdlib / global), so it shows up in *both* base and head outbound sets if `Object.hasOwn` was already used anywhere → no delta.
- The byte-span shifts by ~50 bytes (one extra `if` line) but the symbol that contains the insert is the *enclosing method*, whose anchor span might or might not update depending on how the file's other symbols shifted around it.

The deeper issue: **detection that keys on symbol-level summaries (callee set, byte span) can't see line-level edits.** Some edits genuinely don't change anything summary-level — a sanity-check `if (!Object.hasOwn(x, "k")) return;` is one of them.

## Three candidate solutions

### A) Body content hash on `Symbol` (schema bump)

Add `body_hash` column to the `symbols` table. Computed during indexing as `blake3(source[anchor.start_byte..anchor.end_byte])[..16]`. Stage 4 compares base vs head `body_hash` per stable-ID symbol.

| trait | value |
| --- | --- |
| Catches single-line edits inside body | yes |
| Catches byte-length-preserving edits (rare) | yes |
| Schema bump | yes — `INDEX_FORMAT_VERSION` 2 → 3 |
| Index size growth | ~16 bytes × symbols → MBs on large repos |
| CLI surface change | none |
| Implementation effort | low (~30 LoC + DDL + migration ADR) |

**Score:** clean, fast at review time, captures the most bytes per implementation effort. The schema bump is non-disruptive per ADR-0005 (ephemeral indexes rebuild on demand).

### B) Git diff hunk overlap (no schema bump, needs diff input)

Caller passes git-diff hunk ranges to `mallard pr-review` as JSON:

```json
{ "files": { "lib/adapters/http.js": [{"start": 1294, "end": 1297}, ...] } }
```

For each stable-ID symbol whose anchor overlaps any diff hunk range — even if callees and byte-span are unchanged — emit a `modified-body-touched` comment at `inferred` tier.

| trait | value |
| --- | --- |
| Catches single-line edits inside body | yes (when diff lines provided) |
| Catches edits inferred from index alone | no — needs git diff input |
| Schema bump | no |
| Index size growth | none |
| CLI surface change | new `--diff-hunks <path-to-json>` arg |
| GitHub Action work | run `git diff --unified=0` + format hunks as JSON |
| Implementation effort | medium (~80 LoC + action shell pipeline + tests) |

**Side benefit:** cite exact changed-line range in the comment body, matching the reviewer mental model ("you changed lines 1294-1297; that's inside `Foo.bar`").

**Score:** precise (no false positives from spurious byte-span shifts), but adds an external input dependency. Useless when running `mallard pr-review` outside the GitHub Action.

### C) Read source at review time, hash slices on demand

`mallard pr-review --base-repo <path> --head-repo <path>` reads file bytes from both SHAs (via git worktrees or stored blobs the caller provides). For each stable-ID symbol with no callee delta + no anchor-span delta, hash the source slice at both SHAs and compare.

| trait | value |
| --- | --- |
| Catches single-line edits inside body | yes |
| Catches byte-length-preserving edits | yes |
| Schema bump | no |
| Index size growth | none |
| CLI surface change | new repo-path args, requires git access |
| Implementation effort | medium-high (git plumbing, byte-range reads, error paths) |
| Reproducibility | the duckdb-only contract weakens — review depends on having matching repo state on disk |

**Score:** matches Option A's coverage with no schema bump but breaks the "duckdb is the artifact" contract from ADR-0005 + ADR-0009. Run-time complexity higher.

## Decision

**Ship Option B (diff hunk overlap) first.** Then revisit Option A if Option B's coverage is insufficient on a 100-PR corpus.

Rationale:

- Option B adds **strictly more information** to the review (the precise changed-line range), not just a signal. Comments become more useful even on PRs that already fire callee-delta or anchor-span-delta — they can cite the exact hunk too.
- The GitHub Action is the dominant invocation path. It already has git access; running `git diff --unified=0 base..head` is one command.
- The CLI gains a portable optional input. Local users who want the same behaviour generate the JSON once and pass it.
- **No schema bump** keeps mallard format-stable for the Move 2 benchmark publication. Index reuse across competitor runs matters there.
- If Option B's residual coverage gap (e.g. byte-equivalent semantic edits) becomes a real problem in the full benchmark, Option A is additive — body_hash works orthogonally to hunk overlap.

## Implementation sketch — Option B

### CLI changes

```rust
// src/main.rs — PrReviewArgs
#[arg(long = "diff-hunks")]
diff_hunks: Option<PathBuf>,
```

### pr_review.rs additions

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct DiffHunks {
    pub files: HashMap<String, Vec<DiffRange>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DiffRange {
    pub start: u32,
    pub end: u32,
}

// In review_file, after the callee + span checks both miss:
if let Some(hunks) = diff_hunks.as_ref().and_then(|h| h.files.get(path)) {
    let overlap = hunks.iter().any(|r|
        r.start <= sym.anchor.end_line && r.end >= sym.anchor.start_line
    );
    if overlap {
        // emit `modified-body-touched` comment with the overlapping range
    }
}
```

### Action change

```bash
# .github/actions/review/action.yml — new step before "Run pr-review"
git diff --unified=0 "$BASE_SHA" "$HEAD_SHA" \
  | python3 .github/actions/review/diff-to-hunks.py \
  > .mallard/diff-hunks.json

mallard pr-review ... --diff-hunks .mallard/diff-hunks.json ...
```

Or a small Rust helper inside mallard so no Python dep in the action. Lean to inline-Rust to keep the toolchain matrix tight.

### Tests

- Synthetic fixture: file with a method whose body changes only by a single inserted line, callees identical, byte-span identical (use whitespace padding to preserve span). Diff hunks JSON marks the new line. Assert `modified-body-touched` fires.
- Counter-test: no diff hunks → no `modified-body-touched` emitted (graceful no-op when caller doesn't pass the optional arg).

### Estimated effort

- 1 day for the Rust side (CLI flag + JSON parsing + intersection check + tests)
- 0.5 day for the action-side diff-to-hunks helper + smoke against axios #10901
- 0.5 day to validate against mallard-eval pilot corpus (axios #10901 should flip from 0 → 1 comment)

Total: ~2 days. Lands as a single PR with a clear before/after metric (pilot v2 → pilot v3).

## What this does NOT solve

- True semantic-equivalent edits that swap one token for another with the same byte width AND don't touch the line set (essentially never happens in real PRs).
- Diffs where the entire file is rewritten (no stable-ID symbol exists in both base and head). Stage 3 already handles those via the added/removed lists.

## Backlog status

Option B shipped (PR #36) before the pilot re-run. Phase D / LLM-soft synthesis path subsequently killed per [ADR-0013](../decisions/0013-kill-phase-d-pivot-agent-verification.md).
