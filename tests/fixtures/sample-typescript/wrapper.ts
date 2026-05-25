// Regression fixtures for the TypeScript port of wedge-dogfood-1 gaps.
// Parsed by tree-sitter only — semantically-invalid call sites are
// intentional demonstrations of resolver behaviour.

export class Inner {
  n: number = 7;
  ping(): number {
    return this.n;
  }
}

export class Outer {
  inner: Inner = new Inner();

  ping(): number {
    // Gap 2 / C4 analog: receiver is `this.inner` (Inner), not bare `this`
    // (Outer). Must NOT resolve to Outer.ping itself.
    return this.inner.ping();
  }

  echo(): number {
    // Bare-this method call → same-class resolution, confidence Extracted.
    return this.ping();
  }
}

export class OnlyMethod {
  solo(): number {
    return 7;
  }
}

export function bareSoloMustNotResolveToMethod(): number {
  // C2 analog: `solo` exists only as `OnlyMethod.solo` (Method). Bare-name
  // call cannot reach an instance method without a receiver. The fixture's
  // semantic invalidity is intentional.
  return solo();
}
