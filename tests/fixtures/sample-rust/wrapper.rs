pub struct Inner {
    n: u32,
}

impl Inner {
    pub fn ping(&self) -> u32 {
        self.n
    }
}

pub struct Outer {
    inner: Inner,
}

impl Outer {
    pub fn ping(&self) -> u32 {
        // Receiver is `self.inner` (type Inner), not bare `self` (type Outer).
        // Before ADR-0010 hardening, the extractor matched the short name
        // `ping` against this file's per-name map and asserted confidence
        // Extracted on a wrong target (Outer::ping itself — a self-recursion
        // claim). The fix forces such calls Unresolved at the parser layer
        // so the post-build resolver can mark them Ambiguous (two `ping`
        // candidates in this file) or Inferred (one cross-file callable).
        self.inner.ping()
    }

    pub fn echo(&self) -> u32 {
        // Receiver is bare `self`. Resolves to a same-impl-block method
        // with confidence Extracted — the intended Extracted use.
        self.ping()
    }
}

// Regression for C4: inherent impl + trait impl share method `Foo::tag`.
// Both candidates have qualified_name `Outer::tag` (enclosing_impl_type
// resolves the impl's `type` field for both forms). Without dedupe by
// qualified_name, matching.len()==2 → Unresolved, regressing Extracted.
pub trait Tagged {
    fn tag(&self, suffix: u32) -> u32;
}

impl Tagged for Outer {
    fn tag(&self, suffix: u32) -> u32 {
        100 + suffix
    }
}

impl Outer {
    pub fn tag(&self) -> u32 {
        42
    }

    pub fn show_tag(&self) -> u32 {
        // Bare-self call to `tag`. Two `Outer::tag` symbols exist
        // (inherent + trait). The dedupe-by-qualified_name fix collapses
        // them to one Extracted target instead of regressing.
        self.tag()
    }
}

// Regression for C2: a `&self` method whose short name does not exist as
// a free function in the file must NEVER appear as Extracted from a bare
// callsite — bare-name calls cannot reach a method without a receiver.
// Fixture is parsed by tree-sitter only (never compiled); the dangling
// bare `solo()` is intentional.
pub struct OnlyMethod;

impl OnlyMethod {
    pub fn solo(&self) -> u32 {
        7
    }
}

pub fn bare_solo_must_not_resolve_to_method() -> u32 {
    // `solo` exists only as `OnlyMethod::solo` (Method). Before the fix
    // the bare-name branch returned that Method candidate, falsely
    // claiming Extracted with confidence high. After the fix the bare
    // branch filters Method out of the callable set, leaving the edge
    // Unresolved (resolver may then mark Inferred or Ambiguous).
    solo()
}
