// #6719: reading a well-known symbol off the `Symbol` constructor through
// OPTIONAL chaining (`Symbol?.iterator`) must return the same symbol as dot
// access (`Symbol.iterator`). #6676 fixed the computed bracket form
// (`Symbol["iterator"]`); the optional-chain form lowers through a different
// arm (`arm_optchain.rs`) that never routed through the well-known-symbol
// resolver, so it fell through to a generic property read on the `Symbol`
// constructor and returned `undefined`. `Symbol` is a non-nullish global, so
// `Symbol?.iterator` is exactly `Symbol.iterator`.
//
// Downstream impact: a class/object keyed with `[Symbol?.iterator]` was keyed
// under `undefined` and so was not iterable.
//
// Validated byte-for-byte against `node --experimental-strip-types`.

// ── optional dot (`Symbol?.iterator`) === dot access ────────────────────────
console.log(Symbol?.iterator === Symbol.iterator, typeof Symbol?.iterator);
console.log(
  Symbol?.asyncIterator === Symbol.asyncIterator,
  typeof Symbol?.asyncIterator,
);
console.log(
  Symbol?.toPrimitive === Symbol.toPrimitive,
  typeof Symbol?.toPrimitive,
);
console.log(
  Symbol?.hasInstance === Symbol.hasInstance,
  typeof Symbol?.hasInstance,
);
console.log(
  Symbol?.toStringTag === Symbol.toStringTag,
  typeof Symbol?.toStringTag,
);

// ── optional computed key (`Symbol?.["iterator"]`) — string literal ─────────
console.log(
  Symbol?.["iterator"] === Symbol.iterator,
  typeof Symbol?.["iterator"],
);
console.log(
  Symbol?.["species"] === Symbol.species,
  typeof Symbol?.["species"],
);

// ── optional computed key with a runtime key (`Symbol?.[name]`) ─────────────
// Routes through `js_symbol_computed_member`, identity-equal to dot access.
const grab = (name: string): any => Symbol?.[name as any];
for (const k of ["iterator", "asyncIterator", "toPrimitive", "match"]) {
  console.log(k, grab(k) === (Symbol as any)[k], typeof grab(k));
}

// ── a non-well-known key stays undefined ────────────────────────────────────
console.log(typeof Symbol?.["definitelyNotAWellKnownSymbol" as any]);
console.log(typeof grab("notAKnownSymbol"));

// ── the real manifestation: a class computed method key via `?.` ────────────
class R {
  *[Symbol?.iterator]() {
    yield 1;
    yield 2;
  }
}
console.log(JSON.stringify([...new R()])); // spread → [1,2]
const collected: number[] = [];
for (const v of new R()) collected.push(v); // for-of
console.log(collected.join(","));

// an object literal keyed with `[Symbol?.iterator]` is iterable too
const obj = {
  *[Symbol?.iterator]() {
    yield 10;
    yield 20;
    yield 30;
  },
};
console.log([...obj].join(","));
console.log(Math.max(...obj)); // spread into a call

// ── a locally-shadowed `Symbol` must NOT fold — normal scoping wins ─────────
// Every binding form that shadows the global must be honored: `const`, `class`,
// and `function` (the resolver gates on `shadows_unqualified_global`, not just
// `lookup_local`, which alone would miss the class/function declarations).
function shadowedConst(): string {
  const Symbol = { iterator: "local-value" } as any;
  return typeof Symbol?.iterator + ":" + Symbol?.iterator;
}
console.log(shadowedConst()); // node: "string:local-value"

function shadowedClass(): string {
  class Symbol {
    static iterator = "class-static-iter";
  }
  return String(Symbol?.iterator);
}
console.log(shadowedClass()); // node: "class-static-iter"

function shadowedFunc(): string {
  function Symbol() {}
  (Symbol as any).iterator = "fn-prop-iter";
  return String(Symbol?.iterator);
}
console.log(shadowedFunc()); // node: "fn-prop-iter"

// ── `?.` still short-circuits a genuinely nullish receiver ──────────────────
const maybe: any = null;
console.log(maybe?.iterator); // undefined
