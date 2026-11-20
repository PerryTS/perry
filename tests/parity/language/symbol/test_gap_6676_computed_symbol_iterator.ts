// #6676: reading a well-known symbol off the `Symbol` constructor through a
// COMPUTED key (`Symbol["iterator"]`, or `Symbol[name]` with a runtime key)
// must return the same symbol as dot access (`Symbol.iterator`). Perry only
// intercepted the dot form at HIR lowering, so the bracket form fell through to
// a generic property read and returned `undefined`.
//
// This is exactly what breaks esbuild's `__knownSymbol` helper —
// `(symbol = Symbol[name]) ? symbol : Symbol.for("Symbol." + name)` — that its
// `yield*` / `for-of` / spread downleveling emits for `--target=es2015|es2017`:
// keyed under `undefined`, the delegate iterator dies with "Cannot read
// properties of undefined (reading 'next')".
//
// Validated byte-for-byte against `node --experimental-strip-types`.

// ── string-literal computed key === dot access ──────────────────────────────
console.log(Symbol["iterator"] === Symbol.iterator, typeof Symbol["iterator"]);
console.log(
  Symbol["asyncIterator"] === Symbol.asyncIterator,
  typeof Symbol["asyncIterator"],
);
console.log(
  Symbol["toPrimitive"] === Symbol.toPrimitive,
  typeof Symbol["toPrimitive"],
);
console.log(
  Symbol["hasInstance"] === Symbol.hasInstance,
  typeof Symbol["hasInstance"],
);
console.log(
  Symbol["toStringTag"] === Symbol.toStringTag,
  typeof Symbol["toStringTag"],
);

// ── dynamic (runtime) key — the shape esbuild's __knownSymbol emits ──────────
// Form A: bare `Symbol` receiver, cast only on the key (esbuild's exact shape).
const __knownSymbol = (name: string, symbol?: any): any =>
  (symbol = Symbol[name as any]) ? symbol : Symbol.for("Symbol." + name);
console.log(__knownSymbol("iterator") === Symbol.iterator);
console.log(__knownSymbol("asyncIterator") === Symbol.asyncIterator);

// Form B: cast on the receiver (what a TS author writes) — exercises the
// transparent-wrapper unwrap so `(Symbol as any)[name]` resolves too.
const grab = (name: string): any => (Symbol as any)[name];
const keys = ["iterator", "asyncIterator", "toPrimitive", "match", "species"];
for (const k of keys) {
  console.log(k, grab(k) === (Symbol as any)[k], typeof grab(k));
}

// ── a non-well-known key stays undefined (so the ?: fallback fires) ──────────
console.log(typeof Symbol["definitelyNotAWellKnownSymbol" as any]);
const fallback = __knownSymbol("notAKnownSymbol");
console.log(typeof fallback, (fallback as symbol).description);

// ── the real manifestation: esbuild's es2015 __generator lowering ───────────
// esbuild rewrites generators into PLAIN objects whose `[Symbol.iterator]` is
// SET via `__knownSymbol("iterator")`, then consumed by spread / for-of. Before
// the fix the SET keyed the method under `undefined`, so the consumer (which
// looks up the real `Symbol.iterator`) never found it.
function makeIter(values: number[]): any {
  let i = 0;
  const obj: any = {};
  obj[__knownSymbol("iterator")] = function () {
    return this;
  };
  obj.next = function () {
    return i < values.length
      ? { value: values[i++], done: false }
      : { value: undefined, done: true };
  };
  return obj;
}
console.log(JSON.stringify([...makeIter([1, 2, 3])])); // spread
const collected: number[] = [];
for (const v of makeIter([4, 5])) collected.push(v); // for-of
console.log(collected.join(","));
console.log(Math.max(...makeIter([7, 9, 2]))); // spread into a call

// computed `Symbol.iterator` also resolves the built-in iterator of an array
const arr = [10, 20, 30];
const iterFn = (arr as any)[Symbol["iterator"]].bind(arr);
const viaArray: number[] = [];
for (const v of { [Symbol.iterator]: iterFn }) viaArray.push(v);
console.log(viaArray.join(","));
