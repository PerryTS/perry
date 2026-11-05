// #5835 — Intl service constructor mechanics: `new.target` requirement and
// `Reflect.construct`-driven prototype selection.
//
// ECMA-402 step 1 for ListFormat/RelativeTimeFormat/Segmenter/PluralRules/
// Locale is "If NewTarget is undefined, throw a TypeError exception" — unlike
// the legacy factory-pattern NumberFormat/DateTimeFormat/Collator, which
// silently `new`-ed on a bare call. And OrdinaryCreateFromConstructor means
// `Reflect.construct(Intl.X, args, Custom)` must install `Custom.prototype`
// on the result, not `Intl.X.prototype`. Both must match
// `node --experimental-strip-types` byte-for-byte.
function probe(name: string, fn: () => unknown): string {
  try {
    fn();
    return "no throw";
  } catch (e) {
    return (e as Error).constructor.name;
  }
}

const I: any = (globalThis as any).Intl;

console.log("ListFormat()", probe("ListFormat", () => I.ListFormat()));
console.log("RelativeTimeFormat()", probe("RelativeTimeFormat", () => I.RelativeTimeFormat()));
console.log("Segmenter()", probe("Segmenter", () => I.Segmenter()));
console.log("PluralRules()", probe("PluralRules", () => I.PluralRules()));
console.log("PluralRules.call(undefined)", probe("PluralRules.call", () => I.PluralRules.call(undefined)));
console.log("Locale()", probe("Locale", () => I.Locale()));
console.log("Locale('en')", probe("Locale('en')", () => I.Locale("en")));

// Legacy factory-pattern constructors still allow a bare call (no `new`).
console.log("NumberFormat bare call", probe("NumberFormat", () => I.NumberFormat("en")));
console.log("DateTimeFormat bare call", probe("DateTimeFormat", () => I.DateTimeFormat("en")));
console.log("Collator bare call", probe("Collator", () => I.Collator("en")));

// `Reflect.construct(Intl.X, args, Custom)` installs `Custom.prototype`.
{
  const custom: any = new Function();
  custom.prototype = {};
  const obj = Reflect.construct(I.DisplayNames, [undefined, { type: "language" }], custom);
  console.log("DisplayNames custom prototype", Object.getPrototypeOf(obj) === custom.prototype);
}
{
  const custom: any = new Function();
  custom.prototype = {};
  const obj = Reflect.construct(I.Segmenter, [], custom);
  console.log("Segmenter custom prototype", Object.getPrototypeOf(obj) === custom.prototype);
}

// Ordinary (non-Reflect.construct) instances still resolve the default prototype.
console.log("plain ListFormat prototype", Object.getPrototypeOf(new I.ListFormat("en")) === I.ListFormat.prototype);
console.log("plain Locale prototype", Object.getPrototypeOf(new I.Locale("en")) === I.Locale.prototype);
