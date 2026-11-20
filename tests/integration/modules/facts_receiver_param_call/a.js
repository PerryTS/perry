import { use } from "./b.js";
export function make(f, depth) {
  if (depth > 0) return make(f, depth - 1);
  const o = { f, n: 1 };
  use(o, 2);
  return typeof o.n + ":" + (o.n + 1);
}
