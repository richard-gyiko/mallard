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
