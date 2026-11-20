// #1832: `it.next(v)` across a `yield*` boundary must forward `v` to the
// delegated generator's pending `yield`, matching Node's two-way generator
// communication. The generator transform threads `sent_id` through the
// delegated iterator's `.next(sent)` call (see `linearize.rs`, `next_call_resumed`).
//
// Sibling tests: #1831 (yield* over iterable), #1838 (class
// `[Symbol.iterator]` resolvable via member access). This one isolates
// the resume-value channel so a future change to the driver can't silently
// drop the sent value again.
//
// Validated byte-for-byte against `node --experimental-strip-types`.

// --- one-level delegation: outer.next(v) reaches inner's `yield` ---
function* inner() {
  const r = yield "innerY";
  return "innerRet:" + r;
}
function* g() {
  const a = yield* inner() as any;
  return "got:" + a;
}
const it: any = g();
console.log("step1:", JSON.stringify(it.next()));
console.log("step2:", JSON.stringify(it.next("RESUME")));

// --- multiple resumes across a delegated generator ---
function* counter() {
  let total = 0;
  for (let i = 0; i < 3; i++) {
    const inc = yield i;
    total += typeof inc === "number" ? inc : 0;
  }
  return total;
}
function* outer() {
  const sum = yield* counter() as any;
  return "sum:" + sum;
}
const ot: any = outer();
console.log("c1:", JSON.stringify(ot.next()));
console.log("c2:", JSON.stringify(ot.next(10)));
console.log("c3:", JSON.stringify(ot.next(20)));
console.log("c4:", JSON.stringify(ot.next(30)));

// --- two-level delegation: outer → middle → inner forwards the value ---
function* leaf() {
  const x = yield "leaf";
  return "leaf-ret:" + x;
}
function* mid() {
  const y = yield* leaf() as any;
  return "mid-ret:" + y;
}
function* top() {
  const z = yield* mid() as any;
  return "top-ret:" + z;
}
const tt: any = top();
console.log("t1:", JSON.stringify(tt.next()));
console.log("t2:", JSON.stringify(tt.next("HOP")));

console.log("ALL 1832 YIELD-STAR-RESUME TESTS PASSED");
