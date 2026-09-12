// Gap test for #10090: toLowerCase/toUpperCase gained an ASCII fast path in
// `case_convert` (crates/perry-runtime/src/string/slice_ops.rs) that skips
// the scalar wtf8_step decode / char::to_lowercase()-or-to_uppercase()
// iterator / re-encode loop when every byte of the input is ASCII, doing a
// plain byte-table transform instead.
//
// The fast path must not change behavior for anything it does not apply to.
// This file exercises exactly the boundary cases called out in the issue:
// multi-char Unicode special casing (ß, Cherokee, Deseret, final sigma, the
// default-locale İ special case), a string that is ASCII except for one
// trailing multi-byte character, and an ASCII prefix followed by a lone
// surrogate. This file is byte-compared with `node --experimental-strip-types`
// by the gap suite.

function show(label: string, value: unknown): void {
  console.log(label + ":" + JSON.stringify(value));
}

// Render a string as its UTF-16 code-unit sequence so a lone surrogate
// survives JSON.stringify unambiguously (matches test_gap_9409's `units`).
function units(s: string): number[] {
  return Array.from({ length: s.length }, (_, i) => s.charCodeAt(i));
}

// ---- Pure ASCII: fast-path territory ----
const asciiSamples = [
  "",
  "a",
  "A",
  "aBcD1234EfGh",
  "already lower",
  "ALREADY UPPER",
  "The Quick Brown Fox Jumps Over The Lazy Dog 0123456789 !@#$%^&*()",
  "aBcD".repeat(50),
];
for (const s of asciiSamples) {
  show("ascii-lower:" + JSON.stringify(s), s.toLowerCase());
  show("ascii-upper:" + JSON.stringify(s), s.toUpperCase());
  show("ascii-lower-len:" + JSON.stringify(s), s.toLowerCase().length);
  show("ascii-upper-len:" + JSON.stringify(s), s.toUpperCase().length);
}

// ---- German sharp s: one-to-many default casing, changes .length ----
console.log("sharp-s-upper:" + "straße".toUpperCase()); // "STRASSE"
console.log("sharp-s-upper-len:" + "straße".toUpperCase().length); // 7 (was 6)
console.log("capital-sharp-s-lower:" + "ẞ".toLowerCase()); // "ß"

// ---- Turkic dotted/dotless I must NOT apply under the DEFAULT (non-locale)
// toLowerCase/toUpperCase - that special casing only applies to
// toLocaleLowerCase/toLocaleUpperCase("tr"/"az"), which live in locale.rs and
// are untouched by this fix. ----
console.log("default-i-upper:" + "i".toUpperCase()); // "I"
console.log("default-I-lower:" + "I".toLowerCase()); // "i"

// ---- Default-locale one-to-many special casing (SpecialCasing.txt, locale
// independent): CAPITAL I WITH DOT ABOVE lowercases to "i" + COMBINING DOT
// ABOVE - two code units, distinct from the Turkish-locale mapping. ----
console.log("i-with-dot-lower:" + JSON.stringify("İ".toLowerCase()));
console.log("i-with-dot-lower-units:" + JSON.stringify(units("İ".toLowerCase())));

// NOTE: Greek final sigma (context-dependent Σ -> ς vs σ) is intentionally
// NOT covered here. It is a pre-existing gap in the untouched scalar path
// (Rust's char::to_lowercase() has no notion of the conditional Final_Sigma
// rule) unrelated to the ASCII fast path added by this file's issue, and
// asserting Node's correct output here would fail on main regardless of this
// fix. Tracked separately as #10116.

// ---- Cherokee (Unicode 8.0 added case pairs) ----
console.log("cherokee-lower:" + "Ꭰ".toLowerCase()); // U+AB70
console.log("cherokee-upper:" + "ꭰ".toUpperCase()); // U+13A0

// ---- Deseret (astral, surrogate pair) ----
console.log("deseret-lower:" + "𐐀".toLowerCase()); // U+10428 -> 𐐨
console.log("deseret-lower-units:" + JSON.stringify(units("𐐀".toLowerCase())));
console.log("deseret-upper:" + "𐐨".toUpperCase()); // U+10400 -> 𐐀

// ---- ASCII except for one trailing multi-byte character ----
const asciiPlusOne = "hello" + "é"; // "helloé"
console.log("ascii-plus-one-upper:" + asciiPlusOne.toUpperCase()); // "HELLOÉ"
console.log("ascii-plus-one-lower:" + asciiPlusOne.toLowerCase()); // "helloé"

// ---- ASCII prefix followed by a lone surrogate: must NOT take the ASCII
// fast path (bytes.is_ascii() is false), and the lone surrogate must survive
// verbatim through both directions. ----
const asciiPlusLone = "ABC\ud800";
show("ascii-plus-lone-src-units", units(asciiPlusLone));
show("ascii-plus-lone-lower-units", units(asciiPlusLone.toLowerCase()));
show("ascii-plus-lone-upper-units", units(asciiPlusLone.toUpperCase()));
console.log("ascii-plus-lone-lower-len:" + asciiPlusLone.toLowerCase().length);
console.log("ascii-plus-lone-upper-len:" + asciiPlusLone.toUpperCase().length);
