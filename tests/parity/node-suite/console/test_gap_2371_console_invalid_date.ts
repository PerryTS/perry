// Issue #2371 — `console.log(new Date(NaN))` must print `Invalid Date`, not
// `NaN`. The Invalid-Date sentinel is a quiet NaN carrying a payload that
// marks it as a Date; bundling the value into console.log's argument array
// used to canonicalize the NaN (stripping the payload) so the formatter could
// no longer tell it apart from a bare numeric NaN. These all compare
// byte-for-byte against `node --experimental-strip-types`.

// top-level, inline
console.log(new Date(NaN));
// top-level, via a variable (same value path as instanceof, which already worked)
const d = new Date(NaN);
console.log(d);
// invalid via unparseable string
console.log(new Date("not a date"));

// nested forms (already correct — guard against regression)
console.log([new Date(NaN)]);
console.log({ d: new Date(NaN) });

// a real numeric NaN must still print NaN
console.log(NaN);
console.log([NaN]);

// valid dates must be unaffected
console.log(new Date(0));
const v = new Date(0);
console.log(v);
