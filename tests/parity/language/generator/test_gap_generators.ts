// Test: Generator and Iterator features (Perry gap analysis)
// These features are NOT yet supported by Perry — this file documents the target behavior.
// Run: node --experimental-strip-types tests/parity/language/generator/test_gap_generators.ts

// --- Basic function* with yield ---
function* basicGen(): Generator<number> {
  yield 1;
  yield 2;
  yield 3;
}

const g1 = basicGen();
console.log(g1.next().value); // 1
console.log(g1.next().value); // 2
console.log(g1.next().value); // 3
console.log(g1.next().done);  // true

// --- Generator .next(), .return(), .throw() ---
function* controllableGen(): Generator<number, string, undefined> {
  try {
    yield 10;
    yield 20;
    yield 30;
  } catch (e: any) {
    console.log("caught: " + e.message); // caught: test error
  }
  return "done";
}

const g2 = controllableGen();
console.log(g2.next().value);          // 10
const retResult = g2.return("early");
console.log(retResult.value);          // early
console.log(retResult.done);           // true

const g3 = controllableGen();
g3.next(); // advance to first yield
g3.throw(new Error("test error"));     // prints "caught: test error"

// --- yield* delegation ---
function* inner(): Generator<number> {
  yield 4;
  yield 5;
}

function* outer(): Generator<number> {
  yield 1;
  yield 2;
  yield 3;
  yield* inner();
  yield 6;
}

const delegated: number[] = [];
for (const v of outer()) {
  delegated.push(v);
}
console.log(delegated.join(",")); // 1,2,3,4,5,6

// --- Generator as iterable (for...of) ---
function* rangeGen(start: number, end: number): Generator<number> {
  for (let i = start; i < end; i++) {
    yield i;
  }
}

const rangeResult: number[] = [];
for (const n of rangeGen(0, 5)) {
  rangeResult.push(n);
}
console.log(rangeResult.join(",")); // 0,1,2,3,4

// --- Infinite generator with early break ---
function* naturals(): Generator<number> {
  let n = 0;
  while (true) {
    yield n++;
  }
}

const first5: number[] = [];
for (const n of naturals()) {
  if (n >= 5) break;
  first5.push(n);
}
console.log(first5.join(",")); // 0,1,2,3,4

// --- Generator with return value ---
function* genWithReturn(): Generator<number, string> {
  yield 1;
  yield 2;
  return "final";
}

const g4 = genWithReturn();
console.log(g4.next().value); // 1
console.log(g4.next().value); // 2
const last = g4.next();
console.log(last.value);      // final
console.log(last.done);       // true

// --- Two-way communication (passing values to next()) ---
function* twoWay(): Generator<string, void, number> {
  const a: number = yield "first";
  console.log("received: " + a); // received: 10
  const b: number = yield "second";
  console.log("received: " + b); // received: 20
}

const g5 = twoWay();
console.log(g5.next().value);    // first
console.log(g5.next(10).value);  // second (also prints "received: 10")
g5.next(20);                     // prints "received: 20"

// --- Custom iterable via Symbol.iterator ---
class Range {
  start: number;
  end: number;
  constructor(start: number, end: number) {
    this.start = start;
    this.end = end;
  }

  *[Symbol.iterator](): Generator<number> {
    for (let i = this.start; i < this.end; i++) {
      yield i;
    }
  }
}

const customRange: number[] = [];
for (const n of new Range(3, 7)) {
  customRange.push(n);
}
console.log(customRange.join(",")); // 3,4,5,6

// --- Spread on generator ---
function* spreadGen(): Generator<number> {
  yield 10;
  yield 20;
  yield 30;
}
const spreadResult = [...spreadGen()];
console.log(spreadResult.join(",")); // 10,20,30

// --- Array.from on generator ---
function* arrayFromGen(): Generator<number> {
  yield 100;
  yield 200;
  yield 300;
}
const fromResult = Array.from(arrayFromGen());
console.log(fromResult.join(",")); // 100,200,300

// --- Destructuring from generator ---
function* destructGen(): Generator<number> {
  yield 1;
  yield 2;
  yield 3;
  yield 4;
  yield 5;
}
const [a, b, c, ...rest] = destructGen();
console.log(a);              // 1
console.log(b);              // 2
console.log(c);              // 3
console.log(rest.join(",")); // 4,5

console.log("ALL GENERATOR TESTS PASSED");
