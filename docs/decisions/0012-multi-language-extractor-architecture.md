# 0012 Multi-language extractor architecture

## Status

Accepted

## Context

Move 1 lands Python and TypeScript extractors alongside the original
Rust extractor. The structural pipeline (`SymbolExtractor` trait, edge
emission, post-build resolver, ADR-0010 confidence tiers) is unchanged,
but the per-language details vary on three axes:

1. **Qualified-name separator.** Rust uses `::`; Python and TypeScript
   use `.`. Affects how short names are extracted in both the extractor
   (`symbols_by_short`) and the post-build resolver (`store.rs`).
2. **Bare-receiver token.** Rust: `self`. Python: `self` and `cls`.
   TypeScript: `this`. Gap 2 / C2 / C4 from the wedge-dogfood-1 fixes
   all hinge on detecting bare-receiver method calls.
3. **Constructor-filter heuristic.** Rust's `Foo()` is often a
   tuple-struct constructor; Python's `Foo()` is a class constructor;
   TS uses `new Foo()` (less ambiguous, but `Foo(x)` in JSX or factory
   patterns still triggers the heuristic). The PascalCase fallback and
   the "type-kind" set differ per language.

Naive duplication: copy the Rust extractor file three times, swap
strings. That's ~1500 LOC of triplicated code with three different
maintenance burdens.

Naive abstraction: a `LanguageStrategy` trait with 20 methods. Premature
generalisation; locks in the wrong axes.

## Decision

Extract a thin shared module `src/extractor_common.rs` that hosts the
language-agnostic invariants, and keep per-language extractors as
focused files that only encode language-specific logic.

Shared (`extractor_common.rs`, ~188 LOC):
- `node_text`, `node_anchor` — pure tree-sitter Node helpers.
- `pick_name_and_def`, `canonical_params` — query-match → symbol shape.
- `find_enclosing_definition` — caller-symbol lookup by anchor span.
- `symbols_by_short` — per-file short-name map (separator-parameterised).
- `unique`, `impl_type_prefix` — pure helpers.
- `pick_method_target` — bare-receiver method dispatch + qualified-name
  dedupe (Gap 2 + C4 invariants).
- `is_constructor_call` — PascalCase + `_` heuristic + type-kind
  predicate (passed as fn pointer per language).
- `LangSyntax { qname_sep }` — the language's qualified-name separator.

Per-language (`extractor.rs`, `extractor_python.rs`, `extractor_typescript.rs`):
- Tree-sitter query string (definitions + references).
- Bare-receiver text check (`"self"` / `["self", "cls"]` / `"this"`).
- Symbol-kind mapping for the language's grammar nodes (e.g. Python
  class → `SymbolKind::Struct`; TS interface → `SymbolKind::Trait`).
- Language-specific quirks: Rust macro-body walking (ADR-0010 follow-up);
  TS dual-grammar Query handling (one Query per TS/TSX grammar instance).

Cross-cutting fix in the resolver: `store.rs::short_name_for_resolver`
strips both `::` and `.` separators when building the global `by_short`
table. This is the bridge across language conventions; per-symbol
`qualified_name` keeps the original separator so symbol IDs remain
distinct.

The trait `SymbolExtractor` (in `src/extractor.rs`) is the only public
surface for downstream consumers. New languages slot in as:
1. New `src/extractor_<lang>.rs` file
2. New `tree-sitter-<lang>` dep
3. Extension allowlist row in `src/walk.rs::detect_language`
4. Dispatch arm in `src/file_processor.rs::process`
5. Fixture set under `tests/fixtures/sample-<lang>/`
6. Regression tests for Gap 2 / C2 / C4 / constructor filter
   equivalents in `tests/index_query_integration.rs`

## Alternatives considered

### Per-language trait with 20 methods

**Pros**: Strict interface; impossible to forget a step.

**Cons**: Locks in axes (e.g. `bare_receiver_text()`) before we know
which axes vary. Languages with unusual receiver conventions (e.g. Go's
`func (r *Receiver) Method()` where `r` is user-chosen) would force
trait churn. Defer until language #4 lands.

### One big extractor with switch-on-language

**Pros**: Single file to navigate.

**Cons**: ~800 LOC `match` ladder. Per-language tree-sitter Query
literals all live in one place. Painful to grep when debugging a
Rust-specific behaviour vs a Python-specific one.

### Generate extractors from a config DSL

**Pros**: Less hand-written code.

**Cons**: Massive premature investment. Tree-sitter queries are already
the "config DSL"; wrapping them in another layer would obscure rather
than simplify. Defer indefinitely.

## Consequences

Positive:
- Per-language extractor files stay under ~350 LOC each. Easy to grep,
  easy to dogfood per language.
- New language onboarding is a single PR with a clear template:
  scaffolding chunk → symbols chunk → calls + Gap 2/C2/C4 chunk → rules
  YAML → wedge dogfood. Mirrors the Move 1 Phase A cadence.
- Shared invariants (constructor filter shape, confidence tiers,
  qualified-name dedupe) stay enforced via fn-pointer parameters in
  `extractor_common`. Drift between languages is detectable in code
  review.
- The wedge-dogfood-1 gap fixes (Gap 2 / C2 / C4 / C7) are now language-
  agnostic invariants. Each new extractor MUST satisfy them or its
  regression tests fail.

Negative / tradeoffs:
- `extractor_common.rs` is hand-tuned: pass-by-fn-pointer for type-kind
  predicates, slice + closure for separator. Less type-safe than a trait;
  errors surface at runtime via `pick_extracted_target` returning None.
- Adding a fifth invariant axis (e.g. async-fn detection) requires
  changing the shared module signature OR keeping it per-extractor;
  judgement call at the time.
- The resolver bridge (`short_name_for_resolver`) couples the resolver
  to two separator conventions. A third convention (e.g. Go's `pkg.Fn`
  is already `.`, but Java's `pkg/Class.method` uses `/`) would need a
  schema update or a third resolver branch.

## Related

- [ADR-0003](0003-tree-sitter-and-ast-grep-parsing.md) — tree-sitter as
  the parsing layer.
- [ADR-0008](0008-heuristic-name-resolution.md) — per-language
  heuristic resolver.
- [ADR-0010](0010-edge-confidence-tier.md) — confidence tiers that
  apply identically across languages.
- [ADR-0011](0011-deterministic-only-pr-review-v1.md) — PR review
  surface that consumes the cross-language index.
- [docs/research/wedge-dogfood-1.md](../research/wedge-dogfood-1.md) —
  Gap 2 / C2 / C4 / C7 fixes that became cross-language invariants.
- [docs/plans/move-1-python-ts-action.md](../plans/move-1-python-ts-action.md)
  — the rollout of three languages + GitHub Action this ADR captures.
