// #6696 (follow-up to #6676): a GENERATOR object must expose `[Symbol.iterator]`
// through COMPUTED property access, returning a function that yields the
// generator itself. Perry lowers `g()` to a plain `{next,return,throw}` object
// and the sync `%Generator.prototype%` carried no `[Symbol.iterator]`, so
// `gen[Symbol.iterator]` read `undefined` even though `for (x of gen())` worked
// (that path drives the generator's own `.next()` directly).
//
// The end-to-end victim is esbuild's `__yieldStar` helper, emitted for `yield*`
// at `--target=es2015|es2017`: it does `obj = value[Symbol.iterator]()` on the
// delegate generator, so an `undefined` read makes the helper die one step later
// with "Cannot use 'in' operator to search for 'throw' in undefined".
//
// Validated byte-for-byte against `node --experimental-strip-types`.

function* g() {
  yield 1;
  yield 2;
}

// ── the direct symptom ──────────────────────────────────────────────────────
const it: any = g();
console.log(typeof it[Symbol.iterator]); // "function"
console.log(it[Symbol.iterator]() === it); // true
console.log(Symbol.iterator in it); // true

// The bound method drives the same generator (its own `.next()`).
const it2: any = g();
const iterFn = it2[Symbol.iterator];
const self = iterFn.call(it2);
console.log(self === it2); // true
console.log(JSON.stringify(self.next())); // {"value":1,"done":false}
console.log(JSON.stringify([...it2])); // [2] — one value already consumed

// ── the original #6676 e2e: esbuild's __yieldStar over a delegate generator ──
// This is the shape esbuild emits when downleveling `yield*` to es2015/es2017.
function __yieldStar(inner: any) {
  let obj: any;
  let iter =
    typeof Symbol !== "undefined" && Symbol.iterator && inner[Symbol.iterator];
  if (!iter) return inner;
  iter = iter.call(inner);
  const result: number[] = [];
  let step: any;
  while (!(step = iter.next()).done) result.push(step.value);
  return result;
}
function* inner() {
  yield 1;
  yield 2;
}
console.log(JSON.stringify(__yieldStar(inner()))); // [1,2]

// ── native `yield*` (es2020) still works, unchanged ─────────────────────────
function* innerN() {
  yield 1;
  yield 2;
}
function* outerN() {
  yield* innerN();
}
console.log(JSON.stringify([...outerN()])); // [1,2]

// ── plain `for…of` over a generator is unaffected (drives own .next()) ──────
function* nums() {
  yield 10;
  yield 20;
  yield 30;
}
const collected: number[] = [];
for (const n of nums()) collected.push(n);
console.log(collected.join(",")); // 10,20,30

// ── spread / Array.from / Math.max over a generator ─────────────────────────
function* seq() {
  yield 3;
  yield 7;
  yield 5;
}
console.log(JSON.stringify([...seq()])); // [3,7,5]
console.log(JSON.stringify(Array.from(seq()))); // [3,7,5]
console.log(Math.max(...seq())); // 7

// ── the resolved method is callable and reusable across instances ───────────
function* pair() {
  yield "a";
  yield "b";
}
const p1: any = pair();
const p2: any = pair();
console.log(typeof p1[Symbol.iterator] === typeof p2[Symbol.iterator]); // true
console.log([...p1[Symbol.iterator]()].join("")); // ab
console.log([...p2].join("")); // ab
