// Differential fixture for step 4b (region-scoped guards).
//
// Every case here is a region a naive implementation WOULD admit and get
// wrong. An operator between two reads can be a call: `+` on unvouched
// operands has a cold arm that reaches ToPrimitive and therefore user
// valueOf/toString. JS fixes the order -- `O.a + O.b + O.c` is read a, read b,
// ADD, read c, add -- so a region that loads `c` by slot after that add ran,
// or hoists it above, can return a stale value. Nothing crashes; the guard
// held at entry; the answer is simply wrong.
//
// Expected values are node's. A region that hoists without verifying operand
// primitiveness first returns the number in the comment marked WRONG.

// ---- A: mid-region transition via valueOf, delete ------------------------
const A = { a: null, b: 1, c: 7 };
A.a = { valueOf() { delete A.c; return 1; } };
console.log("A delete:", A.a + A.b + A.c);            // NaN   (WRONG: 9)

// ---- A2: mid-region transition via valueOf, plain overwrite --------------
const A2 = { a: null, b: 1, c: 7 };
A2.a = { valueOf() { A2.c = 100; return 1; } };
console.log("A2 overwrite:", A2.a + A2.b + A2.c);     // 102   (WRONG: 9)

// ---- A3: the same through toString on the SECOND operand -----------------
const A3 = { a: 1, b: null, c: 7 };
A3.b = { toString() { delete A3.c; return "2"; } };
console.log("A3 toString:", A3.a + A3.b + A3.c);      // "12undefined"

// ---- A4: relational operator, not arithmetic -----------------------------
const A4 = { a: null, b: 5, c: 1 };
A4.a = { valueOf() { delete A4.c; return 9; } };
console.log("A4 rel:", (A4.a < A4.b) + ":" + A4.c);   // "false:undefined"

// ---- A5: a getter added mid-region --------------------------------------
const A5 = { a: null, b: 1, c: 7 };
A5.a = { valueOf() { Object.defineProperty(A5, "c", { get() { return 50; } }); return 1; } };
console.log("A5 accessor:", A5.a + A5.b + A5.c);      // 52    (WRONG: 9)

// ---- B: a store inside the run, then a later bail ------------------------
// If a region spans the store and a later operand fails its check, restarting
// the generic copy from the top increments `n` twice.
const B = { n: 10, m: 2, x: null };
B.x = { valueOf() { return 3; } };
let bt = B.n;
B.n = bt + 1;
const bu = B.m + B.x;
console.log("B once:", B.n, bu);                      // 11 5   (WRONG: 12 5)

// ---- B2: the store targets a key the run also reads ----------------------
const B2 = { n: 10, m: 2 };
let b2t = B2.n;
B2.n = b2t + 1;
console.log("B2 same key:", B2.n + B2.m);             // 13     (WRONG: 12)

// ---- C: a Proxy receiver -------------------------------------------------
const CT = { a: 1, b: 2, c: 3 };
let ctrap = 0;
const C = new Proxy(CT, { get(t, k) { ctrap++; return Reflect.get(t, k); } });
console.log("C proxy:", C.a + C.b + C.c, "traps:", ctrap);   // 6 traps: 3

// ---- D: accessor on the prototype, read mid-run --------------------------
const DP = {};
Object.defineProperty(DP, "c", { get() { return 40; }, configurable: true });
const D = Object.create(DP);
D.a = 1; D.b = 2;
console.log("D proto getter:", D.a + D.b + D.c);      // 43

// ---- E: the honest control — all operands primitive ----------------------
// This is the case a region MUST take. No operator can run user code, so
// hoisting the loads is unobservable and the answer is the same either way.
const E = { a: 1, b: 2, c: 3, d: 4 };
let esum = 0;
for (let i = 0; i < 3; i++) esum += E.a + E.b + E.c + E.d;
console.log("E control:", esum);                      // 30

console.log("done");
