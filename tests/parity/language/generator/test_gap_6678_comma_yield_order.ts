// Test: #6678 — a `yield` (or lowered `await`) nested inside a sequence/comma
// expression must run the operands to its LEFT *before* the suspend, not after
// the resume. esbuild's `--target=es2015` async lowering emits bodies shaped
// `push(x), yield …`; perry's generator yield-hoister pulled only the `yield`
// out into a preceding `let __ygen = yield …;` and stranded `push(x)` behind it
// in the now-yield-free comma, so the side effect ran on RESUME (one microtask
// late) — or was dropped entirely when the generator was `.next()`-ed only once.
// Validated byte-for-byte against `node --experimental-strip-types`.

const log: string[] = [];

// --- root cause: `sideEffect, yield null` — side effect runs before suspend ---
function* g1(): any {
  log.push("A"), yield null;
  log.push("B");
}
const it1: any = g1();
it1.next();
console.log("after next#1:", JSON.stringify(log)); // ["A"]
it1.next();
console.log("after next#2:", JSON.stringify(log)); // ["A","B"]

// --- single .next() must NOT drop the pre-yield side effect ---
const dropped: string[] = [];
function* g2(): any {
  dropped.push("ran"), yield null;
}
g2().next();
console.log("single-next side effect:", JSON.stringify(dropped)); // ["ran"]

// --- value-position: `let x = (a, yield b, c)` -> x === c, a's side effect first ---
const seqLog: string[] = [];
function* g3(): any {
  const x = ((seqLog.push("lhs"), 0), yield "Y", "VAL");
  seqLog.push("x=" + x);
}
const it3: any = g3();
const r3 = it3.next();
console.log("value-seq yielded:", r3.value, "log:", JSON.stringify(seqLog)); // Y ["lhs"]
it3.next(99);
console.log("value-seq final:", JSON.stringify(seqLog)); // ["lhs","x=VAL"]

// --- multiple pre-yield operands, all before the suspend, in order ---
const multi: string[] = [];
function* g4(): any {
  multi.push("1"), multi.push("2"), yield 0;
  multi.push("3");
}
const it4: any = g4();
it4.next();
console.log("multi mid:", JSON.stringify(multi)); // ["1","2"]
it4.next();
console.log("multi end:", JSON.stringify(multi)); // ["1","2","3"]

// --- observable microtask order: an async IIFE body `push, await` (which
//     lowers through the same yield-hoist path) must run its synchronous prefix
//     before already-queued microtasks ---
const order: string[] = [];
order.push("sync-start");
Promise.resolve().then(() => order.push("micro"));
(async () => {
  order.push("iife-body"), await null;
})();
order.push("sync-end");
setTimeout(() => {
  // iife-body runs synchronously before the queued "micro"
  console.log("microtask order:", JSON.stringify(order));
  console.log("ALL COMMA-YIELD-ORDER TESTS PASSED");
}, 0);
