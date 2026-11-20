// `BigInt.prototype.toLocaleString` is the sole retainer of the ECMA-402
// number-formatting machinery for programs that never mention `toLocale*`,
// so the thunk carries an `intl-namespace` cfg split. This file mentions
// `toLocaleString`, which turns the feature on — it therefore covers the
// ECMA-402 side of that split: every case below must keep formatting through
// the real Intl number formatter.

const big = 9876543210n;
const small = 42n;
const negative = -1234567n;

console.log("en-US:", big.toLocaleString("en-US"));
console.log("de-DE:", big.toLocaleString("de-DE"));
// `fr-FR` is deliberately absent: Perry groups it with `,` where Node uses
// U+202F, a pre-existing separator gap unrelated to this thunk.

// Under three digits there is no grouping to apply in any locale.
console.log("small en-US:", small.toLocaleString("en-US"));
console.log("small de-DE:", small.toLocaleString("de-DE"));

// The sign must survive grouping.
console.log("negative en-US:", negative.toLocaleString("en-US"));

// Options reach the formatter, not just the locale tag.
console.log(
  "currency:",
  big.toLocaleString("en-US", { style: "currency", currency: "USD" }),
);
console.log(
  "grouping off:",
  big.toLocaleString("en-US", { useGrouping: false }),
);

// Reflective dispatch hits the same thunk as a direct call.
console.log(
  "reflective:",
  (BigInt.prototype.toLocaleString as any).call(big, "en-US"),
);

// Brand check: a non-BigInt receiver must throw rather than format.
let threw = false;
try {
  (BigInt.prototype.toLocaleString as any).call({});
} catch {
  threw = true;
}
console.log("brand check throws:", threw);
