import { performance } from "node:perf_hooks";
// performance.mark(Symbol()) must throw — a Symbol cannot be coerced to a
// string. (Node throws a TypeError.)
try {
  performance.mark(Symbol("s") as any);
  console.log("threw: false");
} catch (e) {
  console.log("threw:", e instanceof TypeError);
}
