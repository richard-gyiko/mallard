import { Counter, double } from "./lib.js";

export function main() {
  const c = new Counter();
  c.bump();
  console.log(double(c.count));
}
