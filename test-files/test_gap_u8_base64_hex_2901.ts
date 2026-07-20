// #2901: TC39 Uint8Array base64/hex conversion APIs.

// Instance toBase64 / toHex round-trips.
console.log(new Uint8Array([72, 105]).toBase64());
console.log(new Uint8Array([72, 105]).toHex());
console.log(new Uint8Array([1, 2, 3]).toBase64());
console.log(new Uint8Array([1, 2]).toBase64());
console.log(new Uint8Array([255, 16, 0]).toHex());

// base64url alphabet + omitPadding.
console.log(new Uint8Array([251, 255, 190]).toBase64({ alphabet: "base64url" }));
console.log(new Uint8Array([1, 2]).toBase64({ alphabet: "base64url" }));
console.log(new Uint8Array([1, 2]).toBase64({ omitPadding: true }));
console.log(new Uint8Array([1, 2]).toBase64({ alphabet: "base64url", omitPadding: true }));

// Static fromBase64 / fromHex.
const a = Uint8Array.fromBase64("SGk=");
console.log(JSON.stringify([...a]));
const b = Uint8Array.fromBase64("AQID");
console.log(JSON.stringify([...b]));
const c = Uint8Array.fromHex("4869");
console.log(JSON.stringify([...c]));
const d = Uint8Array.fromHex("0aff");
console.log(JSON.stringify([...d]));

// base64url decode.
console.log(JSON.stringify([...Uint8Array.fromBase64("-_--", { alphabet: "base64url" })]));

// Empty inputs.
console.log(JSON.stringify([...Uint8Array.fromBase64("")]));
console.log(JSON.stringify([...Uint8Array.fromHex("")]));

// Whitespace tolerance.
console.log(JSON.stringify([...Uint8Array.fromBase64("AQ ID")]));

// setFromBase64 / setFromHex partial writes.
const c1 = new Uint8Array(4);
const r1 = c1.setFromBase64("AQIDBAU=");
console.log(JSON.stringify([...c1]), JSON.stringify(r1));

const c2 = new Uint8Array(5);
const r2 = c2.setFromBase64("AQIDBAU=");
console.log(JSON.stringify([...c2]), JSON.stringify(r2));

const d1 = new Uint8Array(3);
const r3 = d1.setFromHex("0aff10aa");
console.log(JSON.stringify([...d1]), JSON.stringify(r3));

const d2 = new Uint8Array(2);
const r4 = d2.setFromHex("0aff10");
console.log(JSON.stringify([...d2]), JSON.stringify(r4));

// Full round-trip.
const orig = new Uint8Array([0, 1, 2, 250, 251, 255]);
console.log(JSON.stringify([...Uint8Array.fromBase64(orig.toBase64())]));
console.log(JSON.stringify([...Uint8Array.fromHex(orig.toHex())]));

// #6674: the methods must be readable as property VALUES (not just callable),
// so feature-detection (`Uint8Array.prototype.toBase64 ? native : fallback`, as
// jose/Auth.js do) sees them. These read "undefined" pre-fix even though the
// call form dispatched.
console.log(typeof Uint8Array.prototype.toBase64);
console.log(typeof Uint8Array.prototype.toHex);
console.log(typeof Uint8Array.prototype.setFromBase64);
console.log(typeof Uint8Array.prototype.setFromHex);
console.log(typeof Uint8Array.fromBase64);
console.log(typeof Uint8Array.fromHex);
console.log("in:", "toBase64" in Uint8Array.prototype, "toHex" in Uint8Array.prototype);
console.log(
  "detect:",
  typeof Uint8Array.prototype.toBase64 === "function" &&
    typeof Uint8Array.fromBase64 === "function",
);

// Spec `.length` (own arity) of the reified methods.
console.log(
  "len:",
  Uint8Array.prototype.toBase64.length,
  Uint8Array.prototype.toHex.length,
  Uint8Array.prototype.setFromBase64.length,
  Uint8Array.prototype.setFromHex.length,
  Uint8Array.fromBase64.length,
  Uint8Array.fromHex.length,
);

// Negative controls: instance methods are not statics, static factories are not
// on the prototype (must stay "undefined" — the fix must not over-install).
console.log(typeof (Uint8Array as any).toBase64, typeof (Uint8Array.prototype as any).fromBase64);

// The reified prototype value dispatches on reflective `.call` too (brand-checks
// `this`). Compare against the instance form to avoid hardcoding encodings.
const rv = new Uint8Array([0, 1, 2, 250, 255]);
console.log(Uint8Array.prototype.toHex.call(rv) === rv.toHex());
console.log(Uint8Array.prototype.toBase64.call(rv) === rv.toBase64());

// Rebound static factory (`const f = Uint8Array.fromBase64; f(...)`).
const reboundFromB64 = Uint8Array.fromBase64;
const reboundFromHex = Uint8Array.fromHex;
console.log(JSON.stringify([...reboundFromB64("AQID")]));
console.log(JSON.stringify([...reboundFromHex("0aff")]));
