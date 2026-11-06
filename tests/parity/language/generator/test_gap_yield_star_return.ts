// Test: `return yield* inner` delegation in RETURN position (#34).
// The generator linearizer treated `return yield* X` like `return yield X`,
// ignoring the delegate flag: it yielded `X` itself once and returned the
// caller-sent value instead of driving `X`'s iterator protocol and returning
// the delegated iterator's completion value. This surfaced as effect's
// "BUG: yieldWrapGet" for `return yield* Effect.succeed(...)` /
// `return yield* Ref.get(ref)` inside `Effect.gen`, because the bare effect
// (not its `[Symbol.iterator]`-produced YieldWrap) reached the consumer.
// Validated byte-for-byte against `node --experimental-strip-types`.

// --- return yield* drives the inner iterator and returns its completion ---
function* inner1() {
  yield "a";
  yield "b";
  return "innerDone";
}
function* outer1() {
  return yield* inner1() as any;
}
const it1: any = outer1();
console.log(JSON.stringify(it1.next())); // {"value":"a","done":false}
console.log(JSON.stringify(it1.next())); // {"value":"b","done":false}
console.log(JSON.stringify(it1.next())); // {"value":"innerDone","done":true}

// --- the resume value still flows into the delegated generator ---
function* echo() {
  const x = yield "ask";
  return "got:" + x;
}
function* wrap() {
  return yield* echo() as any;
}
const it2: any = wrap();
console.log(JSON.stringify(it2.next()));        // {"value":"ask","done":false}
console.log(JSON.stringify(it2.next("REPLY"))); // {"value":"got:REPLY","done":true}

// --- delegating to an empty-yield generator returns its value immediately ---
function* onlyReturn() {
  return 42;
}
function* wrap3() {
  return yield* onlyReturn() as any;
}
const it3: any = wrap3();
console.log(JSON.stringify(it3.next())); // {"value":42,"done":true}

console.log("ALL YIELD-STAR-RETURN TESTS PASSED");
