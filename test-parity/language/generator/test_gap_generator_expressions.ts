// Test: Generator function EXPRESSIONS (`const g = function*(){}`) — #321/#29.
// Distinct from named `function* g(){}` declarations (test_gap_generators.ts):
// a generator bound to a const/let lowers to `Expr::Closure { is_generator }`
// and is driven through the captures-aware generator transform. Validated
// byte-for-byte against `node --experimental-strip-types`.

// --- .next() driving with sent values + return value ---
const g = function* () {
  const x = yield 1;
  const y = yield 2;
  return (x ?? 0) + (y ?? 0) + 100;
};
const it: any = g();
console.log(JSON.stringify(it.next()));   // {"value":1,"done":false}
console.log(JSON.stringify(it.next(10))); // {"value":2,"done":false}
console.log(JSON.stringify(it.next(20))); // {"value":130,"done":true}

// --- for-of over a generator expression ---
const range = function* () {
  yield 1;
  yield 2;
  yield 3;
};
let sum = 0;
for (const n of range()) sum += n;
console.log("for-of:", sum); // for-of: 6

// --- spread over a generator expression ---
console.log("spread:", [...range()].join(",")); // spread: 1,2,3

// --- yield* delegation to another generator expression ---
const inner = function* () {
  yield "a";
  yield "b";
};
const outer = function* () {
  yield* inner();
  yield "c";
};
console.log("yield*:", [...outer()].join(",")); // yield*: a,b,c

// --- generator expression capturing an outer variable ---
const base = 100;
const adder = function* () {
  yield base + 1;
  yield base + 2;
};
console.log("capture:", [...adder()].join(",")); // capture: 101,102

// --- let-bound (not just const) ---
let counter = function* () {
  yield 7;
  yield 8;
};
console.log("let:", [...counter()].join(",")); // let: 7,8

// --- generator expression inside a function body ---
function makeGen() {
  const local = function* () {
    yield 41;
    yield 42;
  };
  return [...local()];
}
console.log("nested:", makeGen().join(",")); // nested: 41,42

console.log("ALL GENERATOR-EXPRESSION TESTS PASSED");
