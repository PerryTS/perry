// Function.prototype.apply(thisArg, argArray): per CreateListFromArrayLike,
// a non-nullish argArray whose Type is not Object is a TypeError. null and
// undefined mean "no arguments" and call through cleanly. Primitives —
// including a Symbol, which is heap-backed but still a primitive — must throw.
// test262: built-ins/Function/prototype/apply/argarray-not-object.js
function fn(...args: unknown[]): number {
  return args.length;
}

function probe(label: string, run: () => unknown): void {
  try {
    const result = run();
    console.log(label, "ok", typeof result, String(result));
  } catch (e: unknown) {
    const ctor = e && (e as { constructor?: { name?: string } }).constructor;
    console.log(label, "threw", ctor ? ctor.name : typeof e);
  }
}

// Nullish argArray → no arguments, runs clean.
probe("null", () => fn.apply(null, null));
probe("undefined", () => fn.apply(null, undefined));

// Non-object primitives → TypeError.
probe("boolean", () => fn.apply(null, true as never));
probe("number", () => fn.apply(null, NaN as never));
probe("string", () => fn.apply(null, "1,2,3" as never));
probe("symbol", () => fn.apply(null, Symbol() as never));
probe("bigint", () => fn.apply(null, 10n as never));

// Real array-likes still spread correctly.
probe("array", () => fn.apply(null, [1, 2, 3]));
probe("arraylike", () => fn.apply(null, { length: 2, 0: "a", 1: "b" } as never));
