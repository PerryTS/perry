// Strict equality against a Symbol whose constructor provenance is known can
// use raw identity. Keep dynamic values, module globals, fresh/registered
// Symbols, and annotation lies together so the optimization cannot silently
// widen beyond the representation contract it proved.

const MISSING: any = Symbol("missing");

function isMissing(value: any): boolean {
  return value === MISSING;
}

console.log(
  isMissing(MISSING),
  isMissing(Symbol("missing")),
  isMissing({}),
  isMissing([]),
  isMissing("missing"),
  isMissing(undefined),
);

const fresh = Symbol();
console.log(fresh === fresh, fresh !== Symbol(), Symbol() === Symbol());

const registeredA = Symbol.for("perry-symbol-identity-equality");
const registeredB = Symbol.for("perry-symbol-identity-equality");
const registeredOther = Symbol.for("perry-symbol-identity-equality-other");
console.log(
  registeredA === registeredB,
  registeredA !== registeredOther,
  isMissing(registeredA),
);

// TypeScript annotations are erased and therefore are not runtime evidence.
// This must keep generic object identity, including after the array grows and
// an alias may retain the pre-grow forwarding address.
let notReallyASymbol: symbol = [] as any;
const oldAlias: any = notReallyASymbol as any;
for (let i = 0; i < 128; i++) {
  (notReallyASymbol as any).push(i);
}
console.log(notReallyASymbol === (oldAlias as any));

// Loose equality has coercion rules and deliberately stays on the runtime
// helper. The wrapper converts to the exact primitive Symbol.
const symbolWrapper = {
  [Symbol.toPrimitive](): symbol {
    return MISSING;
  },
};
console.log((MISSING as any) == (symbolWrapper as any));
