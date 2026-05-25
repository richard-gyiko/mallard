## What

<!-- One sentence on what this PR does. -->

## Why

<!-- The problem this solves. Link the issue if there is one. -->

## Checklist

- [ ] `cargo build --release --locked` passes
- [ ] `cargo test --release --locked` passes
- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy --release --all-targets -- -D warnings` passes
- [ ] If public CLI / JSON shape changed: `docs/cli-json-contract.md` updated and `schema_version` bumped where breaking
- [ ] If domain language changed: `CONTEXT.md` updated
- [ ] If design choice is non-trivial: new ADR under `docs/decisions/`
- [ ] No LLM integration added (see ADR-0013)
