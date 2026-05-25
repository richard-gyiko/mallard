"""Regression fixtures for the Python port of wedge-dogfood-1 gaps.

Same patterns as `tests/fixtures/sample-rust/wrapper.rs`. Parsed by
tree-sitter only — semantically-invalid call sites are intentional
demonstrations of resolver behaviour, not runnable code.
"""


class Inner:
    def __init__(self):
        self.n = 7

    def ping(self):
        return self.n


class Outer:
    def __init__(self):
        self.inner = Inner()

    def ping(self):
        # Gap 2: receiver is `self.inner` (different type from Outer).
        # Must NOT resolve to Outer.ping itself; the resolver tiers as
        # Ambiguous (two `ping` methods in this file).
        return self.inner.ping()

    def echo(self):
        # Bare-self receiver: same-class call. Resolves to Outer.ping
        # with confidence Extracted.
        return self.ping()


class OnlyMethod:
    def solo(self):
        return 7


def bare_solo_must_not_resolve_to_method():
    # C2 analog: `solo` exists only as `OnlyMethod.solo` (Method).
    # Bare-name call cannot reach an instance method without a receiver.
    return solo()  # noqa — intentional unresolved
