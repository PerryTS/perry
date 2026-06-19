// Reflect.apply(target, thisArgument, argumentsList): per spec
// sec-reflect.apply the argumentsList is run through CreateListFromArrayLike,
// which throws a TypeError when Type(argumentsList) is not Object. Unlike
// Function.prototype.apply (where a nullish argArray means "no arguments"),
// Reflect.apply rejects EVERY non-object argumentsList — null, undefined, and
// primitives all throw — and an OMITTED argumentsList is `undefined`, so it
// must throw too rather than silently calling with zero arguments.
// test262: built-ins/Reflect/apply/arguments-list-is-not-array-like.js
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

const R = Reflect as unknown as {
  apply: (t: unknown, thisArg: unknown, argsList?: unknown) => unknown;
};

// Non-object argumentsList → TypeError (no special nullish handling).
probe("null", () => R.apply(fn, null, null));
probe("undefined", () => R.apply(fn, null, undefined));
probe("omitted", () => R.apply(fn, null));
probe("boolean", () => R.apply(fn, null, true));
probe("number", () => R.apply(fn, null, 1));
probe("nan", () => R.apply(fn, null, NaN));
probe("infinity", () => R.apply(fn, null, Infinity));
probe("symbol", () => R.apply(fn, null, Symbol()));

// Real array / array-like argumentsList still spreads correctly.
probe("array", () => R.apply(fn, null, [1, 2, 3]));
probe("arraylike", () => R.apply(fn, null, { length: 2, 0: "a", 1: "b" }));
