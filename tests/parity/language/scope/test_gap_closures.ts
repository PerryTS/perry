// Gap test: Closure capture and invocation
// Run: node --experimental-strip-types test_gap_closures.ts

// --- Basic capture ---
const x = 10;
const f1 = () => x;
console.log("basic capture:", f1());

// --- Capture multiple variables ---
const a = 1;
const b = "hello";
const c = true;
const f2 = () => a + " " + b + " " + c;
console.log("multi capture:", f2());

// --- Capture in nested function ---
function outer(n: number): () => number {
  return () => n * 2;
}
const double5 = outer(5);
console.log("nested capture:", double5());
console.log("nested capture 2:", outer(10)());

// --- Mutable capture ---
let counter = 0;
const inc = () => { counter++; return counter; };
console.log("inc 1:", inc());
console.log("inc 2:", inc());
console.log("inc 3:", inc());
console.log("counter:", counter);

// --- Closure over loop variable (let) ---
const fns: (() => number)[] = [];
for (let i = 0; i < 5; i++) {
  fns.push(() => i);
}
const loopResults: number[] = [];
for (let i = 0; i < fns.length; i++) {
  loopResults.push(fns[i]());
}
console.log("let loop capture:", loopResults);

// --- Closure returning closure ---
function adder(n: number): (m: number) => number {
  return (m: number) => n + m;
}
const add5 = adder(5);
console.log("curried:", add5(3));
console.log("curried:", add5(10));

// --- Closure with default params ---
function withDefault(n: number = 42): () => number {
  return () => n;
}
console.log("default param:", withDefault()());
console.log("override param:", withDefault(99)());

// --- Closure in array methods ---
const nums = [1, 2, 3, 4, 5];
const factor = 3;
const scaled = nums.map((n: number) => n * factor);
console.log("map with capture:", scaled);

const threshold = 3;
const filtered = nums.filter((n: number) => n > threshold);
console.log("filter with capture:", filtered);

const prefix = "item_";
const prefixed = nums.map((n: number) => prefix + n);
console.log("map with string capture:", prefixed);

// --- Closure capturing object ---
const obj = { x: 10, y: 20 };
const getSum = () => obj.x + obj.y;
console.log("capture object:", getSum());

// --- Closure capturing array ---
const arr = [10, 20, 30];
const getFirst = () => arr[0];
const getLast = () => arr[arr.length - 1];
console.log("capture array first:", getFirst());
console.log("capture array last:", getLast());

// --- Immediately invoked closure ---
const result = ((n: number) => n * n)(7);
console.log("IIFE:", result);

// --- Closure as callback ---
function applyTwice(fn: (n: number) => number, val: number): number {
  return fn(fn(val));
}
const doubler = (n: number) => n * 2;
console.log("callback:", applyTwice(doubler, 3));

// --- Multiple closures sharing same captured variable ---
let shared = 0;
const incShared = () => { shared++; return shared; };
const getShared = () => shared;
incShared();
incShared();
console.log("shared after inc:", getShared());
incShared();
console.log("shared after more:", getShared());

// --- Closure capturing class instance ---
class Counter {
  count: number;
  constructor() { this.count = 0; }
  increment(): void { this.count++; }
  getCount(): number { return this.count; }
}

const ctr = new Counter();
const doIncrement = () => { ctr.increment(); return ctr.getCount(); };
console.log("class capture 1:", doIncrement());
console.log("class capture 2:", doIncrement());
console.log("class capture 3:", doIncrement());

// --- Deeply nested closures ---
function level1(a: number): () => () => () => number {
  return () => {
    return () => {
      return () => a * 10;
    };
  };
}
console.log("deep nested:", level1(5)()()());
