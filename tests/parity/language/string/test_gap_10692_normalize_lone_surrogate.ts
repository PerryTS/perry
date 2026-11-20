// #10692: `String.prototype.normalize` / `localeCompare` on a WTF-8 payload
// holding a lone surrogate.
//
// Perry stores strings as WTF-8, so "\uD800" is representable on the heap as
// three bytes that are NOT valid UTF-8. The runtime used to hand those bytes
// straight to `str::from_utf8_unchecked` (undefined behaviour), and the output
// was correct only because `unicode-normalization` happens to tolerate an
// out-of-range `char`. This fixture pins the observable behaviour against Node
// so the guarded implementation cannot drift.
//
// Every string is BUILT FROM CODE UNITS and every result is PRINTED as code
// units. No literal non-ASCII character appears in this file: a precomposed
// character pasted where a decomposed sequence was meant is invisible on
// screen and silently turns a normalization test into a no-op.

function cu(...codeUnits: number[]): string {
  return String.fromCharCode(...codeUnits);
}

function units(s: string): string {
  const out: string[] = [];
  for (let i = 0; i < s.length; i++) out.push(s.charCodeAt(i).toString(16));
  return "[" + out.join(", ") + "]";
}

const E = 0x65; // LATIN SMALL LETTER E
const ACUTE = 0x301; // COMBINING ACUTE ACCENT
const E_ACUTE = 0xe9; // LATIN SMALL LETTER E WITH ACUTE (precomposed)
const HI = 0xd800; // lone high surrogate
const LO = 0xdc00; // lone low surrogate
const FI = 0xfb01; // LATIN SMALL LIGATURE FI
const ANGSTROM = 0x212b; // ANGSTROM SIGN (singleton decomposition)
const PUA = 0xe000; // first private-use code point, just above the surrogates
const SIGMA = 0x3a3; // GREEK CAPITAL LETTER SIGMA

const inputs: string[] = [
  cu(HI),
  cu(LO),
  cu(E, ACUTE, HI), // decomposed, composes before the surrogate
  cu(E_ACUTE, HI), // precomposed
  cu(HI, E_ACUTE),
  cu(E, HI, ACUTE), // the surrogate is a starter: must NOT compose across it
  cu(FI, HI), // ligature expands under the K forms
  cu(LO, E_ACUTE),
  cu(0x61, HI, 0x62),
  cu(HI, ACUTE),
  cu(ANGSTROM, HI),
  cu(HI, HI),
  cu(SIGMA, HI), // Final_Sigma context broken by the surrogate
];
const forms: string[] = ["NFC", "NFD", "NFKC", "NFKD"];

console.log("=== normalize ===");
for (const s of inputs) {
  for (const f of forms) {
    console.log(units(s) + " " + f + " -> " + units(s.normalize(f)));
  }
}

console.log("=== normalize default form ===");
for (const s of inputs) {
  console.log(units(s) + " -> " + units(s.normalize()));
}

console.log("=== normalize preserves well-formedness reporting ===");
for (const s of inputs) {
  const n = s.normalize();
  console.log(units(s) + " wf=" + n.isWellFormed() + " len=" + n.length);
}

// The *form* argument is user-controlled too, so it can itself hold a lone
// surrogate. `e.name + ":" + e.message` is the idiom the #2782 fixture uses.
function normalizeForm(subject: string, form: any): string {
  try {
    return units(subject.normalize(form));
  } catch (e: any) {
    return e.name + ":" + e.message;
  }
}
console.log("=== invalid forms still throw RangeError ===");
console.log(normalizeForm(cu(HI), "BAD"));
console.log(normalizeForm(cu(HI), null));
console.log(normalizeForm(cu(HI), ""));
console.log(normalizeForm("a", cu(HI)));
console.log(normalizeForm(cu(HI), cu(LO)));

const pairs: string[][] = [
  [cu(HI), cu(HI)],
  [cu(HI), cu(LO)],
  [cu(HI), "a"],
  ["a", cu(HI)],
  [cu(HI), ""],
  ["", cu(HI)],
  [cu(HI), cu(PUA)],
  [cu(0x61, HI), cu(0x61, HI)],
  [cu(0x61, HI), cu(0x62, HI)],
  [cu(HI, 0x61), cu(HI, 0x62)],
  // Canonical equivalence across a surrogate: decomposed vs precomposed.
  [cu(E, ACUTE, HI), cu(E_ACUTE, HI)],
  [cu(HI, E, ACUTE), cu(HI, E_ACUTE)],
];

function sign(n: number): string {
  return n < 0 ? "-1" : n > 0 ? "1" : "0";
}

console.log("=== localeCompare ===");
for (const p of pairs) {
  console.log(units(p[0]) + " vs " + units(p[1]) + " -> " + sign(p[0].localeCompare(p[1])));
}

console.log("=== localeCompare numeric ===");
for (const p of pairs) {
  const r = p[0].localeCompare(p[1], undefined, { numeric: true });
  console.log(units(p[0]) + " vs " + units(p[1]) + " -> " + sign(r));
}

console.log("=== toLowerCase / toUpperCase keep the surrogate ===");
for (const s of inputs) {
  console.log(units(s) + " -> " + units(s.toLowerCase()) + " / " + units(s.toUpperCase()));
}

console.log("=== toWellFormed is the only U+FFFD substituter ===");
for (const s of inputs) {
  console.log(units(s) + " wf=" + s.isWellFormed() + " -> " + units(s.toWellFormed()));
}
