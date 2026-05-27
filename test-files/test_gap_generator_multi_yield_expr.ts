// Test: multiple `yield` / `yield*` inside a single (non-statement,
// non-simple-binding) expression. Each yield must be hoisted to an ordered
// temp, the generator suspended/resumed at each, and the resumed values
// combined left-to-right (#321 Effect.gen general-case enabler). Previously a
// yield buried in `a + b` / call args / array literals fell into the generator
// linearizer's catch-all and lowered via the "generators not implemented"
// codegen arm (returns 0.0) — the resumed value was dropped and the generator
// never suspended at the buried yields.
// Validated byte-for-byte against `node --experimental-strip-types`.

// --- two yields in arithmetic, resumed values combined ---
function* g(): any {
  return (yield 1) + (yield 2);
}
const it: any = g();
console.log(JSON.stringify(it.next())); // {"value":1,"done":false}
console.log(JSON.stringify(it.next(10))); // {"value":2,"done":false}
console.log(JSON.stringify(it.next(20))); // {"value":30,"done":true}

// --- three yields ---
function* h(): any {
  return (yield 1) + (yield 2) + (yield 3);
}
const i2: any = h();
i2.next();
i2.next(10);
i2.next(20);
console.log("3yield:", i2.next(30).value); // 60

// --- yields as array-literal elements (evaluation order) ---
function* arr(): any {
  return [yield 1, yield 2];
}
const a: any = arr();
a.next();
a.next(11);
console.log("array:", JSON.stringify(a.next(22).value)); // [11,22]

// --- yields as call arguments ---
function add3(x: number, y: number, z: number): number {
  return x + y + z;
}
function* call(): any {
  return add3(yield 1, yield 2, yield 3);
}
const c: any = call();
c.next();
c.next(5);
c.next(6);
console.log("call:", c.next(7).value); // 18

// --- && short-circuit: RHS yield must NOT run when LHS falsy ---
function* andSc(): any {
  return false && (yield 99);
}
const as1: any = andSc();
const asr = as1.next();
console.log("and-short:", asr.value, asr.done); // false true

// --- && where LHS truthy: RHS yield runs and its resume value flows out ---
function* andRun(): any {
  return true && (yield 42);
}
const ar: any = andRun();
const arA = ar.next();
const arB = ar.next(100);
console.log("and-run:", arA.value, arB.value); // 42 100

// --- || short-circuit: RHS yield must NOT run when LHS truthy ---
function* orSc(): any {
  return 7 || (yield 99);
}
const os: any = orSc();
const osr = os.next();
console.log("or-short:", osr.value, osr.done); // 7 true

// --- ?? : RHS runs only when LHS is null/undefined ---
function* coalRun(): any {
  return null ?? (yield 5);
}
const co: any = coalRun();
const coA = co.next();
const coB = co.next(123);
console.log("coal-run:", coA.value, coB.value); // 5 123

function* coalSc(): any {
  return 8 ?? (yield 5);
}
const cs: any = coalSc();
const csr = cs.next();
console.log("coal-short:", csr.value, csr.done); // 8 true

// --- ternary: only the taken branch's yield runs ---
function* tern(cond: boolean): any {
  return cond ? (yield 1) : (yield 2);
}
const tt: any = tern(true);
const ttA = tt.next();
const ttB = tt.next(50);
console.log("tern-true:", ttA.value, ttB.value); // 1 50
const tf: any = tern(false);
const tfA = tf.next();
const tfB = tf.next(60);
console.log("tern-false:", tfA.value, tfB.value); // 2 60

// --- yield* inside an expression (delegated completion values combined) ---
function* inner(n: number): any {
  const r = yield n;
  return r + n;
}
function* deleg(): any {
  return (yield* inner(1)) + (yield* inner(2));
}
const d: any = deleg();
console.log(JSON.stringify(d.next())); // {"value":1,"done":false}
console.log(JSON.stringify(d.next(10))); // {"value":2,"done":false}
console.log(JSON.stringify(d.next(20))); // {"value":33,"done":true}

console.log("ALL MULTI-YIELD-EXPR TESTS PASSED");
