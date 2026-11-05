// Gap test for String semantics parity:
//  #2786 / #2880 — padStart/padEnd length limits (throw, not clamp)
//  #2782        — normalize() invalid-form RangeError
//  #2789        — callable String.raw(callSite, ...subs)
//
// Compared byte-for-byte against `node --experimental-strip-types`.

// ---- padStart ----
function ps(len: number): string {
  try {
    return JSON.stringify("x".padStart(len, "0"));
  } catch (e: any) {
    return e.name + ":" + e.message;
  }
}
function pe(len: number): string {
  try {
    return JSON.stringify("x".padEnd(len, "0"));
  } catch (e: any) {
    return e.name + ":" + e.message;
  }
}
for (const len of [0, 2, 5, -1, NaN, Infinity, 2 ** 53 - 1, 2 ** 32]) {
  console.log("padStart", len, "=>", ps(len));
}
for (const len of [0, 2, 5, -1, NaN, Infinity, 2 ** 53 - 1, 2 ** 32]) {
  console.log("padEnd", len, "=>", pe(len));
}
// Empty filler / already-long-enough short-circuit before the length check.
console.log("padStart Infinity empty filler =>", JSON.stringify("x".padStart(Infinity, "")));
console.log("padStart 2 over len-5 =>", JSON.stringify("hello".padStart(2, "0")));

// ---- normalize ----
function nf(f: any): string {
  try {
    return JSON.stringify("é".normalize(f));
  } catch (e: any) {
    return e.name + ":" + e.message;
  }
}
console.log("norm none =>", JSON.stringify("é".normalize()));
console.log("norm NFC =>", nf("NFC"));
console.log("norm NFD =>", nf("NFD"));
console.log("norm NFKC =>", nf("NFKC"));
console.log("norm NFKD =>", nf("NFKD"));
console.log("norm BAD =>", nf("BAD"));
console.log("norm null =>", nf(null));
console.log("norm empty =>", nf(""));

// ---- callable String.raw ----
function raw1(): string {
  try {
    return JSON.stringify(String.raw({ raw: ["a", "b"] }, "X"));
  } catch (e: any) {
    return e.name + ":" + e.message;
  }
}
function raw2(): string {
  try {
    return JSON.stringify(String.raw({ raw: ["a", "b", "c"] }, "X", "Y"));
  } catch (e: any) {
    return e.name + ":" + e.message;
  }
}
function raw3(): string {
  try {
    return JSON.stringify(String.raw({ raw: { 0: "a", 1: "b", length: 2 } } as any, "X"));
  } catch (e: any) {
    return e.name + ":" + e.message;
  }
}
function raw4(): string {
  try {
    return JSON.stringify(String.raw({ raw: [] }, "X"));
  } catch (e: any) {
    return e.name + ":" + e.message;
  }
}
function raw5(): string {
  try {
    return JSON.stringify(String.raw({ raw: { length: 0 } } as any, "X"));
  } catch (e: any) {
    return e.name + ":" + e.message;
  }
}
function raw6(): string {
  try {
    return JSON.stringify(String.raw({ raw: null } as any, "X"));
  } catch (e: any) {
    return e.name + ":" + e.message;
  }
}
function raw7(): string {
  try {
    return JSON.stringify(String.raw(null as any, "X"));
  } catch (e: any) {
    return e.name + ":" + e.message;
  }
}
function raw8(): string {
  try {
    return JSON.stringify(String.raw({ raw: ["a", "b", "c"] }, "X"));
  } catch (e: any) {
    return e.name + ":" + e.message;
  }
}
console.log(raw1());
console.log(raw2());
console.log(raw3());
console.log(raw4());
console.log(raw5());
console.log(raw6());
console.log(raw7());
console.log(raw8());

// tagged-template form must still work
const name = "world";
console.log(String.raw`hi\n${name}!`);
