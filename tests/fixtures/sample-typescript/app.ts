import { Counter, double } from "./lib";

export function main(): void {
  const c = new Counter();
  c.bump();
  console.log(double(c.count));
}
