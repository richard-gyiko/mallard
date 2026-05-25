// Plain JS exercised by the TS extractor.

export function double(x) {
  return x * 2;
}

export class Counter {
  constructor() {
    this.count = 0;
  }

  bump() {
    this.count += 1;
    return double(this.count);
  }
}
