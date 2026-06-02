// Test: yield*-delegation forwards the resume value (#1832).
// `outer.next(v)` across a `yield*` boundary must deliver `v` to the
// delegated generator's pending `yield` expression. Previously the in-loop
// `delegatedIterator.next()` was called with no argument, dropping `v`.
// Validated byte-for-byte against `node --experimental-strip-types`.

// --- bare yield* (statement position) forwards resume value ---
function* inner1() {
  const a = yield "i1";
  const b = yield "i2";
  return `inner:${a},${b}`;
}
function* outer1() {
  const ret = yield* inner1() as any;
  yield `ret=${ret}`;
}
const it1: any = outer1();
console.log(JSON.stringify(it1.next()));        // {"value":"i1","done":false}
console.log(JSON.stringify(it1.next("A")));     // {"value":"i2","done":false}
console.log(JSON.stringify(it1.next("B")));     // {"value":"ret=inner:A,B","done":false}
console.log(JSON.stringify(it1.next()));        // {"value":undefined→ ... ,"done":true}

// --- const x = yield* inner() captures the inner return; resume flows in ---
function* inner2() {
  const r = yield "y";
  return "innerRet:" + r;
}
function* g2() {
  const a = yield* inner2() as any;
  return "got:" + a;
}
const it2: any = g2();
console.log(JSON.stringify(it2.next()));         // {"value":"y","done":false}
console.log(JSON.stringify(it2.next("RESUME"))); // {"value":"got:innerRet:RESUME","done":true}

// --- without an explicit cast (bare expression) ---
function* inner3() {
  const v = yield 1;
  return v + 10;
}
function* g3() {
  const out = yield* inner3();
  return out;
}
const it3: any = g3();
console.log(JSON.stringify(it3.next()));      // {"value":1,"done":false}
console.log(JSON.stringify(it3.next(5)));     // {"value":15,"done":true}

console.log("ALL YIELD-STAR-RESUME TESTS PASSED");
