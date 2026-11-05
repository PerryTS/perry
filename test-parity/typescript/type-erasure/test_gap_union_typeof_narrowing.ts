// `typeof x === "string"` narrowing on a `string | T[]` union.
// Perry's `is_string_expr` returns true for any `Union` whose members
// include `String` (the nullable-string narrowing case, `s: string |
// null`). When the method itself is array-only (`.join`, `.push`, …),
// blindly routing to `lower_string_method` made the runtime fall into
// the string-method catch-all and throw
// `TypeError: (string).join is not a function` even though the actual
// runtime value was an array — i.e. the else branch of the narrow.
//
// Fix: at the property-method dispatch site, skip the string-method
// route when the method name is on `Array.prototype` but NOT on
// `String.prototype`; runtime dispatch then picks the right path by
// the receiver's actual shape.
//
// Compared byte-for-byte against `node --experimental-strip-types`.

// (1) The original #2277 repro: `string | number[]` narrowed via
//     `typeof === "string"`. The else branch calls `.join(",")` on
//     `input` typed as the still-Union (TS narrowing not in HIR), so
//     it must resolve at runtime.
function processInput(input: string | number[]): string {
  if (typeof input === "string") {
    return input.toUpperCase();
  } else {
    return input.join(",");
  }
}
console.log("(1)", processInput("hello"));
console.log("(1)", processInput([1, 2, 3]));

// (2) Mirror for `string | T[]` with a different array-only method.
//     Confirms the array path covers more than `.join`.
function describe(x: string | number[]): string {
  if (typeof x === "string") {
    return "s:" + x.length;
  } else {
    return "a:" + x.filter((n: number) => n > 0).length;
  }
}
console.log("(2)", describe("hello"));
console.log("(2)", describe([1, -2, 3, -4, 5]));

// (3) Reverse position — the array variant first in the union. The
//     fix must work regardless of variant order.
function joinOrUpper(x: number[] | string): string {
  if (typeof x === "string") {
    return x.toUpperCase();
  }
  return x.join("-");
}
console.log("(3)", joinOrUpper("perry"));
console.log("(3)", joinOrUpper([10, 20, 30]));

// (4) The nullable-string narrowing case must still work — `.toUpperCase`
//     on `string | null` after a null-guard should still route through
//     `lower_string_method`. (Regression check for the existing
//     `is_string_expr` Union arm.)
function maybeUpper(x: string | null): string {
  if (x !== null) {
    return x.toUpperCase();
  }
  return "(null)";
}
console.log("(4)", maybeUpper("ok"));
console.log("(4)", maybeUpper(null));
