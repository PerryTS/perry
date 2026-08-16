// #7981 / #6759 C3c: `ObjectHeader.parent_class_id` is overloaded. For a class
// instance it is the parent class id; for a plain object (`class_id == 0`) the
// same word carries the runtime ShapeId stamp, written lazily by every by-name
// resolve path.
//
// The `perry/thread` serializer used to copy that word verbatim, and the
// worker-side deserializer feeds it to `js_object_alloc_with_parent`, which
// registers it as a class-parent edge — so a plain object that had been READ
// once and then crossed a `spawn` / `parallelMap` boundary registered
// `class 0 -> <a shape id>` in the process-global registry. The serializer now
// reads the edge from the class-parent registry instead.
//
// The discriminating assertion (that the registry is not polluted) is a
// perry-runtime unit test — the registry is not observable from JS. THIS test
// is the behavioural regression half: both object kinds must still round-trip,
// and inheritance must still work on the far side after a stamped plain object
// has crossed first.
//
// perry-only (`perry/thread` has no Node equivalent), so this is an
// `test_issue_*` behavioural test, not a byte-for-byte gap test.
import { parallelMap, spawn } from "perry/thread";

class Base {
  b: number;
  constructor(b: number) {
    this.b = b;
  }
  kind(): string {
    return "base";
  }
}
class Mid extends Base {
  m: number;
  constructor(b: number, m: number) {
    super(b);
    this.m = m;
  }
  kind(): string {
    return "mid";
  }
}
// Fieldless indirect subclass — the shape CLAUDE.md flags as weak.
class Leaf extends Mid {}

// A plain object literal, READ on the main thread first so the resolve path
// stamps a ShapeId into `parent_class_id`. Without the read the word is 0 and
// the test is vacuous, so read it and print the value we read.
const lit: Record<string, number> = { alpha: 1, beta: 2, gamma: 3 };
console.log("stamped read:", lit.beta);

function summarize(o: Record<string, number>): string {
  const keys = Object.keys(o);
  let sum = 0;
  for (const k of keys) sum += o[k];
  return keys.join(",") + "=" + sum;
}

// 1. The stamped plain object crosses first. Pre-fix this is the deserialize
//    that polluted the registry with `class 0 -> <shape id>`.
const mapped = parallelMap([lit, lit, lit, lit], (o: Record<string, number>) =>
  summarize(o),
);
console.log("plain mapped:", mapped.join(" | "));

// 2. Inheritance must still resolve on a worker AFTER that deserialize — the
//    edges the serializer now reads come from the same registry the pollution
//    landed in.
function chain(n: number): string {
  const leaf = new Leaf(n, n * 10);
  const parts: string[] = [
    leaf.kind(),
    String(leaf.b),
    String(leaf.m),
    String(leaf instanceof Mid),
    String(leaf instanceof Base),
    String(new Mid(n, n) instanceof Base),
  ];
  return parts.join(",");
}
const expected = chain(4);
console.log("main chain:", expected);

const chained = parallelMap([4, 4, 4, 4], (n: number): string => chain(n));
let allMatch = true;
for (let i = 0; i < chained.length; i++) {
  if (chained[i] !== expected) allMatch = false;
}
console.log("worker chain count:", chained.length, "allMatch:", allMatch);

// 3. A class INSTANCE crossing the boundary keeps its fields.
const inst = new Mid(7, 70);
const back = await spawn((): string => chain(7));
console.log("spawn chain:", back, "match:", back === chain(7));
console.log("instance fields:", inst.b, inst.m, inst.kind());

// 4. The main thread is still correct after all of it.
console.log("main again:", chain(4) === expected, summarize(lit));
