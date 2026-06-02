// Test: a class's computed well-known-symbol method `[Symbol.iterator]() {}`
// is resolvable via member access (#1838). Class lowering names it `@@iterator`
// in the vtable (not a symbol property), so `instance[Symbol.iterator]`,
// `Symbol.iterator in instance`, and `yield*` delegation must all find it.
// (This is how effect's `EffectPrimitive` exposes its iterator, which
// `Effect.gen`'s `yield* effect` depends on.)
//
// Scope: member access / `in` / `yield*`. for-of and spread over a
// non-generator class iterator go through a separate lowering path and are a
// follow-up. Validated byte-for-byte against `node --experimental-strip-types`.

class Range {
  n: number;
  constructor(n: number) {
    this.n = n;
  }
  [Symbol.iterator]() {
    let i = 0;
    const n = this.n;
    return {
      next: () => (i < n ? { value: i++, done: false } : { value: undefined, done: true }),
    };
  }
}

// --- `Symbol.iterator in instance` + member access ---
const r = new Range(3);
console.log("has:", Symbol.iterator in r); // true
console.log("typeof:", typeof (r as any)[Symbol.iterator]); // function

// --- calling the resolved method yields a working iterator ---
const it = (r as any)[Symbol.iterator]();
console.log("manual:", JSON.stringify(it.next()), JSON.stringify(it.next()), JSON.stringify(it.next()), JSON.stringify(it.next()));
// {"value":0,"done":false} {"value":1,"done":false} {"value":2,"done":false} {"value":...,"done":true}

// --- yield* delegation to a class iterable ---
function* g() {
  yield 100;
  yield* new Range(3) as any;
  yield 200;
}
console.log("yield*:", [...g()].join(",")); // 100,0,1,2,200

// --- inherited [Symbol.iterator] resolves through the class chain ---
class Sub extends Range {}
function* h() {
  yield* new Sub(2) as any;
}
console.log("inherited:", [...h()].join(",")); // 0,1

console.log("ALL CLASS-SYMBOL-ITERATOR TESTS PASSED");
