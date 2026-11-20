import { performance } from "node:perf_hooks";
// performance.clearMarks(Symbol()) must throw — the name argument cannot be a
// Symbol. (Node throws a TypeError.)
try {
  performance.clearMarks(Symbol("f") as any);
  console.log("threw: false");
} catch (e) {
  console.log("threw:", e instanceof TypeError);
}
