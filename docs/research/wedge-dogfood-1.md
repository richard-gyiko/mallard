# Wedge dogfood #1 — BurntSushi/ripgrep PR #3195

First non-mallard PR run through the `pr-review` skill. Goal: surface real consumer-facing gaps vs. synthetic mallard-on-mallard ones.

## PR shape

- Repo: BurntSushi/ripgrep (Rust, ~50 crates monorepo style)
- PR: [#3195](https://github.com/BurntSushi/ripgrep/pull/3195) "searcher: fix regression with `--line-buffered` flag"
- Base: `38d630261aded3a8e535fe85761e68af35bc462d`
- Head: `eb60087486a01f573cae9186fcca8973ef602dfd`
- Diff: 17 added / 19 deleted across 3 files
  - `CHANGELOG.md` (non-Rust, skipped per stage 1 filter)
  - `crates/searcher/src/line_buffer.rs` — body restructure of `LineBuffer::fill` (revert of an inner read-loop)
  - `crates/searcher/src/searcher/glue.rs` — two string-literal updates inside test bodies (`262142` → `262146`)

A small, body-only revert PR. Exactly the shape stage 4 was added for.

## Indexing

| step             | files | symbols | calls edges | wall   |
| ---------------- | ----- | ------- | ----------- | ------ |
| index base       | 100   | 3364    | 10709       | 1.9 s  |
| index head       | 100   | 3364    | 10706       | 1.8 s  |

100/220 files indexed; 120 skipped by extension (no Rust). No parse errors. Indexing itself was uneventful — fast, deterministic, no friction.

## Stage 3 (signature diff)

Both changed files: zero added, zero removed, base==head symbol set. Expected: PR doesn't touch signatures.

## Stage 4 (modified-body via edge diff)

One detection across the whole PR:

```
MODIFIED-BODY LineBuffer::fill id=9af7649f055772e0dc0bdfa0ece31883
  added: [as_bytes_mut]
  removed: (none)
```

Zero detections in `glue.rs`. **First gap.**

## Gaps surfaced

### Gap 1 — String-literal-only body changes are invisible to edge-diff

`glue.rs` had two real changes (test expectation literals `262142` → `262146` at lines 740 and 774). Both lines are inside test function bodies. Neither changes any callee — so stage 4's outbound-edge diff sees nothing.

This is a structural blind spot, not a bug. Stage 4's contract is "shape of what changed, even if the body isn't available". A test fixture flip that doesn't touch any callable is shape-invisible.

**Consequence for synthesis**: a reviewer who only sees stage 3 + stage 4 output would conclude "diff touches no parseable structural changes" for `glue.rs` and skip it entirely. For a test-only literal change that's usually correct (the test exists to enforce the value, no further review needed). But the skill currently has no way to distinguish "I genuinely have nothing to say" from "I'm blind to this change". The early-exit summary lies by omission.

**Possible fixes**:
- Add a stage-4.5b: "files in the diff with no detected symbol-level change" enumerated explicitly in the summary, with the file's diff size, so the reviewer knows mallard is silent rather than absent.
- Future: a `text-anchor diff` primitive that flags any line-range overlap between the unified diff and any indexed symbol's anchor.

### Gap 2 — Self-impl-block call resolution bias (the headline finding)

`LineBufferReader::fill` (line 257) calls `self.line_buffer.fill(&mut self.rdr)`. Its `LineBuffer` field is a different type from `LineBufferReader`, so this should resolve to `LineBuffer::fill`.

Mallard resolved it to **`LineBufferReader::fill` itself**, with confidence `extracted`:

```
=== LineBufferReader::fill outbound ===
  conf=extracted dst=LineBufferReader<'b, R>::fill
```

That's a self-edge (recursion claim) on a method that is not recursive. The resolver appears to bias toward the same-impl-block method when the short name matches and the call is `self.X.<name>(...)`. The receiver path (`self.line_buffer`) carries the type information needed to disambiguate; the resolver ignores it.

Three `fill` symbols exist in the head index:
- `LineBuffer::fill` (the modified function)
- `LineBufferReader::fill` (the wrapper at line 257)
- `ReadByLine::fill` (in glue.rs)

The correct behaviour: at minimum, mark `fill` as `ambiguous` and surface all three candidates. The actively-wrong behaviour is asserting `extracted` confidence on a wrong target — high-confidence false positive, the worst kind of resolver mistake. Stage 5's evidence for the actually-modified `LineBuffer::fill` shows zero inbound callers; `LineBufferReader::fill` is silently swallowing the real edge.

**This is the most important wedge finding so far.** It's exactly the kind of issue [ADR-0010](../decisions/0010-edge-confidence-tier.md) was meant to catch — but the resolver got it wrong with the wrong confidence label. Either:
- The same-impl-block heuristic needs to back off when the receiver is `self.<field>` (i.e. not bare `self`).
- The `extracted` tier needs a stricter definition: a *true* extraction is when the parser recovered the full path inside the file; a same-name same-impl match is not that, it's `inferred` at best.

[[adr-0010-edge-confidence]] reads this finding directly.

### Gap 3 — Method calls inside macro invocations are missed

Tests at lines 603, 611, 615 (etc., 40+ sites) all look like `assert!(rdr.fill().unwrap())`. The parser extracts zero `fill` call edges from any of them.

Demonstration: `tests::buffer_basics1` (lines 595–618) is indexed with 7 outbound `calls` edges, none of which is `fill`, despite the test body containing three `assert!(rdr.fill()...)` lines. `LineBufferBuilder::new`, `consume`, `consume_all` are all picked up (they're outside the `assert!` macro). The `fill` call inside the macro is dropped.

`unresolved-callers --name fill --index head.duckdb` returns **zero results** for the head index. The 40+ test call sites are completely invisible.

**Consequence for synthesis**: stage 5's inbound expansion for `LineBuffer::fill` shows zero callers. A naive synthesis ("this function has no callers in the head index → possibly dead code") would be catastrophically wrong here. The function is called from forty places; the parser just doesn't see them.

The skill's `Gotchas` section already says "no inbound callers" doesn't mean "no callers exist" (per [ADR-0008](../decisions/0008-heuristic-name-resolution.md)). But the wedge surfaces a sharper version: a *common Rust idiom* — chaining method calls inside `assert!`, `assert_eq!`, `expect`, `with_context`, etc. — drops calls wholesale, not just at the unresolved tier. The macro-invocation skip is silent, not surfaced as `dst_unresolved`.

This is a tree-sitter-Rust extractor heuristic question: should the query descend into macro_invocation bodies? In Rust, macro bodies *are* parsed as token trees, but `assert!`'s body happens to be valid expression syntax. The current extractor likely doesn't descend.

**Estimated impact**: any Rust crate with a substantial test suite where tests call functions through `assert!`-family macros will under-count inbound edges. Probably most Rust libraries.

### Gap 4 — Ambiguous-confidence noise dominates real signal

`ReadByLine::fill` outbound (a complex glue.rs method, ~30 calls in body): 15 edges, 11 of them `ambiguous`. Names like `buffer`, `len`, `consume`, `binary_byte_offset`. Most are ambiguous because they're trait-ish names with many candidates across the index.

For PR review synthesis, this is mostly noise. Per [ADR-0010](../decisions/0010-edge-confidence-tier.md) and the skill's "ambiguous edges are highest-priority for human disambiguation" guidance, an LLM operating on this evidence would over-emit "verify which `buffer` this targets" comments. None of them are actually about the PR.

**The wedge insight**: ambiguous-confidence is high-signal **on changed symbols**, where the diff localises attention. Spread across all transitive evidence (stage 5's depth-1 expand on every changed symbol), it's noise. The skill currently doesn't filter "ambiguous edges newly introduced by this PR" vs. "ambiguous edges that have always been there". The latter aren't worth surfacing.

**Possible fix**: an "edge-confidence diff" — only flag ambiguous edges that exist in head but not base. Today's stage 4 only diffs the *set* of callees, not their confidence labels. A `cross_confidence_diff` would catch "this call used to resolve, now it's ambiguous" (likely a resolver regression worth a comment).

## What worked

- Indexing speed is fine on a 100-file Rust crate (~2 s). No reason to expect linear scaling failure up to a few thousand files.
- The skill's stage-1 file filter (`*.rs` only) cleanly handled the mixed-language diff.
- `edges-by-file` with one query per direction made stage 4 trivial; no per-symbol loop needed.
- `unresolved-callers` was fast even though it returned zero hits (the cost is in the SQL, not in the result size).
- The confidence-tier surface is well-shaped — the gap is in the *content* of each tier, not the API.

## Net wedge verdict

**Stage 4 caught the one real edge change (`+as_bytes_mut`).** That's a vindication of stage 4 existing: stage 3 alone would have reported "diff touches no parseable structural changes" and stopped.

**But** the stage 5 evidence for the one detected symbol was unusable. Zero inbound callers (Gap 3, macro-invocation skip), wrong-direction outbound edge to `LineBufferReader::fill` (Gap 2, self-impl bias). A reviewer running the skill would get a single comment of the shape "LineBuffer::fill added a call to `as_bytes_mut`; this method has no callers and probably dead" — both clauses false in actionable ways.

The wedge holds for *detection*. The wedge does not yet hold for *synthesis* on real Rust code with macro-heavy test suites and `self.<field>.<method>` call patterns. Both issues are extractor/resolver-side; neither is a query-primitive issue.

## Suggested next priorities

Compared to next.md's existing priority list:

- **New P0**: fix self-impl-block resolution bias (Gap 2). Specific test case: `crates/searcher/src/line_buffer.rs:257` — `self.line_buffer.fill(...)` should not resolve to `LineBufferReader::fill`. Either back off to `ambiguous` or fix the receiver-type lookup. Smallest-change variant: refuse to claim `extracted` confidence for a self-impl-block match when the call's receiver is `self.<field>` (not bare `self`).
- **New P1**: extractor descends into `assert!`/`assert_eq!`/`expect`/`with_context` macro bodies for `calls` edges (Gap 3). Without this, Rust test-suite inbound recall is broken.
- **Confirm Priority 4 (ambiguous-with-context) is still right**: this wedge does *not* show ambiguous-with-context noticeably improving synthesis, because the ambiguous edges that matter are mostly non-PR-related noise (Gap 4). The bigger win is "newly ambiguous after PR" (an edge that resolved before and now doesn't), which needs an edge-confidence diff in stage 4, not candidate-list enrichment in the edges table.
- **Existing Priority 1 was right**: this dogfood produced a list of real consumer-facing gaps, not synthetic ones. Run #2 (Python or TypeScript wedge once second extractor lands) will tell whether these are Rust-specific or general.

## Process notes for the skill

- Stage 1 filter dropped `CHANGELOG.md` cleanly. Good.
- The skill should explicitly enumerate "diff files with no detected structural change" in the summary, not silently skip. A reviewer needs to know mallard was silent vs. absent (Gap 1 framing).
- The stage-5 evidence dump (depth-1 expand) was usable but noisy. For a single-symbol PR, displaying all 15 outbound edges of every transitively-reached symbol is overkill. Maybe gate depth-1 transitive-node edges behind "this transitive node was also touched by the PR" instead of always expanding everything.
