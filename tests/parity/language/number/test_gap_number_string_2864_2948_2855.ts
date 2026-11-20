// Gap test for #2864 (Number/BigInt toString radix validation + formatting)
// and #2948 (String.prototype.at returns UTF-16 code units, not code points).
//
// #2855 (tagged-template caching + freeze) was DROPPED from this PR: it
// requires per-call-site identity (codegen site-ids) plus making array
// element writes honor the frozen flag — broad multi-subsystem infra.

// ---- #2864: Number.prototype.toString(radix) ----
console.log((255).toString(16)); // ff
console.log((10).toString(2)); // 1010
console.log((255).toString(2)); // 11111111
console.log((10.5).toString(2)); // 1010.1
console.log((10.5).toString(16)); // a.8
console.log((10.5).toString(36)); // a.i
console.log((255).toString(2.9)); // 11111111 (radix truncated to 2)
console.log((-255).toString(16)); // -ff
console.log((-10.5).toString(2)); // -1010.1
console.log((255).toString("16")); // ff (radix string-coerced)

// ---- #2864: BigInt.prototype.toString(radix) ----
console.log((255n).toString(2)); // 11111111
console.log((255n).toString(16)); // ff
console.log((255n).toString("16")); // ff

// ---- #2864: invalid radices throw RangeError (Number) ----
for (const r of [1, 37, 0, NaN, Infinity, "bad"]) {
  try {
    (255).toString(r as any);
  } catch (e: any) {
    console.log("num", e.name);
  }
}

// ---- #2864: invalid radices throw RangeError (BigInt) ----
for (const r of [1, 37, 0, NaN, Infinity, "bad"]) {
  try {
    (255n).toString(r as any);
  } catch (e: any) {
    console.log("big", e.name);
  }
}

// ---- #2948: String.prototype.at is UTF-16 code-unit based ----
const s = "\u{1F600}"; // astral char stored as a surrogate pair
console.log("len", s.length); // 2
// at() returns a single code unit, so its .length is 1 (NOT the whole 2-unit
// emoji that the old code-point-decoding behavior returned). The exact lone
// surrogate code unit value (0xd83d) is the documented WTF-8 categorical gap,
// so we assert the code-unit *count* rather than charCodeAt here.
console.log("at0len", s.at(0)!.length); // 1
console.log("at1len", s.at(1)!.length); // 1
console.log("atneg1len", s.at(-1)!.length); // 1
console.log("atOOB", s.at(5)); // undefined
// codePointAt keeps full code-point semantics as the contrast.
console.log("cp0", s.codePointAt(0)!.toString(16)); // 1f600

// at() on a plain BMP string behaves like charAt with negative index support.
console.log("abc.at1", "abc".at(1)); // b
console.log("abc.at-1", "abc".at(-1)); // c
console.log("abc.at5", "abc".at(5)); // undefined
