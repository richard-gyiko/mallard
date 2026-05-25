export function double(x: number): number {
  return x * 2;
}

export class Counter {
  count: number = 0;

  bump(): number {
    this.count += 1;
    return double(this.count);
  }
}

export interface Named {
  name(): string;
}

export type CounterFactory = () => Counter;
