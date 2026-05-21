# 0010 Edge confidence tier: extracted / inferred / ambiguous / unresolved

## Status

Accepted

## Context

Edges in the index currently exist in two binary states:

- **Resolved**: `dst_symbol_id` is set; the parser or post-build resolver found a target. No record of *which* layer resolved it.
- **Unresolved**: `dst_symbol_id` is NULL and `dst_unresolved` carries the bare name. We don't know *why* it stayed unresolved — was the name absent from the index, ambiguous, or a stdlib call beyond the indexed crate?

Two pieces of evidence push toward a confidence-tier model:

1. [code-review-graph](https://github.com/tirth8205/code-review-graph) ships a three-tier `EXTRACTED / INFERRED / AMBIGUOUS` edge confidence. The reviewer prioritises low-confidence edges (where the tool is less sure) for human attention; high-confidence edges can be trusted at face value.
2. The 2026 research synthesis ([docs/research/agentic-code-review-tools.md](../research/agentic-code-review-tools.md)) names *evidence provenance* and *trust calibration* as open problems. Reviewers need to know *how* a finding was arrived at, not just *what* the finding is. Anthropic, Claude Code Review, and the IRIS/SAILOR systems all surface evidence provenance as a first-class output field.

A third datapoint is internal: dogfooding `pr-review` against PR #7 surfaced ambiguous-name resolution as a silent failure mode. The post-build resolver refuses to pick a winner when multiple callable symbols share a short name — but the user never sees these. They look identical to "stdlib call, can't resolve."

The wedge's first-pass-assistant framing ([ADR-0009](0009-pr-review-architecture-pattern.md)) makes confidence first-class for synthesis: high-confidence-extracted edges anchor the most reliable comments; ambiguous edges become flags for "human disambiguation needed"; unresolved edges (stdlib / external) are downplayed because they're beyond the indexed scope.

## Decision

Add a `confidence` column to the `edges` table with four values:

- **`extracted`** — the parser resolved the target within the same file using the per-file `symbols_by_name` map. Highest confidence; no inference involved.
- **`inferred`** — the post-build resolver matched the target across files via the unambiguous-callable rule. Confident but cross-file; uses a heuristic.
- **`ambiguous`** — the resolver found multiple callable candidates for the name and refused to pick one. The edge is recorded with `dst_symbol_id IS NULL` + `dst_unresolved` set, and `confidence = ambiguous` so reviewers can disambiguate.
- **`unresolved`** — no candidate found anywhere in the index. Almost always stdlib / external crate calls. Lowest confidence.

Existing fields stay:
- `dst_symbol_id` is set iff confidence ∈ {extracted, inferred}.
- `dst_unresolved` is set iff confidence ∈ {ambiguous, unresolved}.

The build runs in three phases that match the tiers:

1. **Parser** (per file) emits edges with `confidence = extracted` when intra-file resolution wins, else `confidence = unresolved` initially.
2. **Resolver** (post-build, before finalize) upgrades unresolved-but-resolvable to `confidence = inferred`. When it sees multiple callable candidates, it writes the edge as `confidence = ambiguous` instead of leaving it as `unresolved`.
3. **Reader** surfaces `confidence` on `NeighborEdge`, `FileEdgeBundle`, `UnresolvedCallerHit`. The `pr-review` skill uses it to prioritise synthesis (ambiguous > inferred > extracted ≫ unresolved for "needs human attention").

Schema bump: `INDEX_FORMAT_VERSION` increments to 2. Per [ADR-0005](0005-ephemeral-indexing-defer-incremental.md) indexes are ephemeral, so the bump is non-breaking for users — they rebuild.

## Alternatives considered

### Keep binary resolved/unresolved

Pros:
- No schema change, no migration.
- Simpler model.

Cons:
- Ambiguous matches stay silently dropped; reviewers never see the cases where the tool *almost* knew the answer.
- No way for the `pr-review` skill to prioritise its synthesis by evidence quality.
- Misses the research-aligned trust-calibration story.

### Two-tier (resolved-deterministic / inferred / unresolved)

Pros:
- Captures the extracted vs inferred distinction without exposing ambiguous separately.

Cons:
- Ambiguous cases continue to be silently dropped, which is the highest-value gap in today's resolver.
- Halfway move that we'd want to extend later anyway.

### Continuous confidence score (0.0–1.0)

Pros:
- More flexible; could combine signals (e.g., short-name match + same-module penalty).

Cons:
- Premature — no consumer needs a fine-grained score. The tiers are categorical signals about *how* the edge was produced, not numeric reliability.
- Harder to act on in the `pr-review` skill (thresholding vs case-matching).

### Four-tier (this decision)

Pros:
- Captures the three observable production paths (intra-file extraction, cross-file inference, multi-match ambiguity) plus the genuinely-unknown case.
- Each tier maps to a synthesis policy in the wedge.
- Surfaces previously-silent ambiguous resolutions — direct dogfood value.

Cons:
- One more column on the edges table.
- Resolver gets slightly more bookkeeping (track ambiguous candidates rather than just skipping them).
- Adds a `EdgeConfidence` enum + serde across the wire.

## Consequences

Positive:
- The `pr-review` skill can rank synthesized comments by edge confidence: ambiguous edges become high-priority "verify which target" prompts; extracted edges anchor low-effort confidence; unresolved edges (stdlib) get de-emphasised so they don't clutter output.
- Ambiguous resolutions become visible. Dogfood evidence on PR #7 will quantify how often they fire — informs whether the heuristic should be tightened.
- The schema change is small and local; no API breakage at the `IndexReader` level (new field on existing types).
- Aligns with the research recommendation that production reviewers need evidence provenance.

Negative / tradeoffs:
- `INDEX_FORMAT_VERSION` bump means every existing index must rebuild. Acceptable per [ADR-0005](0005-ephemeral-indexing-defer-incremental.md).
- The resolver gains complexity (track ambiguous, write a record for them). Bounded — single helper.
- Synthesis prompts in the `pr-review` skill grow a confidence-aware section. Worth it for the trust-calibration gain.
- Future per-language extractors must set `confidence` correctly when emitting edges; one more invariant for each new `SymbolExtractor` impl to satisfy. Document in CONTEXT.md.

## Related

- [0002-duckdb-as-graph-and-index-store.md](0002-duckdb-as-graph-and-index-store.md) — schema is owned by mallard.
- [0005-ephemeral-indexing-defer-incremental.md](0005-ephemeral-indexing-defer-incremental.md) — supports format-version bumps without migration.
- [0008-heuristic-name-resolution.md](0008-heuristic-name-resolution.md) — the resolver this ADR enhances.
- [0009-pr-review-architecture-pattern.md](0009-pr-review-architecture-pattern.md) — the layered pipeline that consumes confidence in synthesis.
- [docs/research/agentic-code-review-tools.md](../research/agentic-code-review-tools.md) — evidence-provenance argument.
