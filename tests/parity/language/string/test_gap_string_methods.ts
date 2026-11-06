// Gap test: String methods not yet supported by Perry
// Run: node --experimental-strip-types test_gap_string_methods.ts

// --- str.at() (positive and negative indexing) ---
const str = "hello world";
console.log("at(0):", str.at(0)); // 'h'
console.log("at(4):", str.at(4)); // 'o'
console.log("at(-1):", str.at(-1)); // 'd'
console.log("at(-5):", str.at(-5)); // 'w'
console.log("at(100):", str.at(100)); // undefined

// Edge cases
const empty = "";
console.log("empty.at(0):", empty.at(0)); // undefined
const single = "x";
console.log("single.at(0):", single.at(0)); // 'x'
console.log("single.at(-1):", single.at(-1)); // 'x'

// --- str.codePointAt() ---
const ascii = "ABC";
console.log("codePointAt(0):", ascii.codePointAt(0)); // 65
console.log("codePointAt(1):", ascii.codePointAt(1)); // 66
console.log("codePointAt(2):", ascii.codePointAt(2)); // 67

// Emoji / surrogate pairs
const emoji = "\u{1F600}"; // grinning face
console.log("emoji codePointAt(0):", emoji.codePointAt(0)); // 128512
console.log("emoji codePointAt(0) hex:", emoji.codePointAt(0)?.toString(16)); // '1f600'

// Multi-codepoint string
const mixed = "A\u{1F600}B";
console.log("mixed codePointAt(0):", mixed.codePointAt(0)); // 65 (A)
console.log("mixed codePointAt(1):", mixed.codePointAt(1)); // 128512 (emoji)
console.log("mixed codePointAt(3):", mixed.codePointAt(3)); // 66 (B)

// Out of bounds
console.log("codePointAt(100):", ascii.codePointAt(100)); // undefined

// --- String.fromCodePoint() ---
console.log("fromCodePoint(65):", String.fromCodePoint(65)); // 'A'
console.log("fromCodePoint(65,66,67):", String.fromCodePoint(65, 66, 67)); // 'ABC'
console.log("fromCodePoint(0x1F600):", String.fromCodePoint(0x1F600)); // grinning face emoji
console.log("fromCodePoint(9731):", String.fromCodePoint(9731)); // snowman

// Multiple codepoints including emoji
const built = String.fromCodePoint(72, 101, 108, 108, 111, 32, 0x1F30D);
console.log("fromCodePoint multi:", built); // 'Hello ' + globe emoji

// --- String.raw ---
const rawStr = String.raw`Hello\nWorld`;
console.log("String.raw:", rawStr); // 'Hello\nWorld' (literal backslash-n, no newline)
console.log("String.raw length:", rawStr.length); // 12

const rawPath = String.raw`C:\Users\test\documents`;
console.log("String.raw path:", rawPath); // 'C:\Users\test\documents'

// String.raw with interpolation
const name = "Perry";
const rawInterp = String.raw`Hello ${name}\n`;
console.log("String.raw interp:", rawInterp); // 'Hello Perry\n'

// --- str.isWellFormed() and str.toWellFormed() ---
if (typeof "".isWellFormed === "function") {
  const wellFormed = "Hello World";
  console.log("isWellFormed normal:", wellFormed.isWellFormed()); // true

  // Lone surrogate (ill-formed)
  const illFormed = "ab\uD800cd";
  console.log("isWellFormed lone surrogate:", illFormed.isWellFormed()); // false

  const fixed = illFormed.toWellFormed();
  console.log("toWellFormed:", fixed.isWellFormed()); // true
  console.log("toWellFormed replaces with U+FFFD:", fixed.includes("\uFFFD")); // true
} else {
  console.log("isWellFormed/toWellFormed: not available");
}

// --- str.normalize() ---
// NFC: Canonical Decomposition followed by Canonical Composition
// NFD: Canonical Decomposition
const accented = "\u00E9"; // e-acute (precomposed)
const decomposed = "\u0065\u0301"; // e + combining acute accent

console.log("precomposed === decomposed:", accented === decomposed); // false
console.log("NFC === NFC:", accented.normalize("NFC") === decomposed.normalize("NFC")); // true
console.log("NFD === NFD:", accented.normalize("NFD") === decomposed.normalize("NFD")); // true

console.log("NFC length:", decomposed.normalize("NFC").length); // 1
console.log("NFD length:", accented.normalize("NFD").length); // 2

// NFKC and NFKD (compatibility)
const fiLigature = "\uFB01"; // fi ligature
console.log("fi ligature NFKC:", fiLigature.normalize("NFKC")); // 'fi'
console.log("fi ligature NFKD:", fiLigature.normalize("NFKD")); // 'fi'
console.log("fi ligature NFC:", fiLigature.normalize("NFC")); // still ligature

// Default normalize() is NFC
console.log("default normalize is NFC:", decomposed.normalize() === decomposed.normalize("NFC")); // true

// --- str.localeCompare() ---
const a = "apple";
const b = "banana";
const c = "apple";

console.log("apple vs banana:", a.localeCompare(b) < 0); // true (apple comes before banana)
console.log("banana vs apple:", b.localeCompare(a) > 0); // true
console.log("apple vs apple:", a.localeCompare(c) === 0); // true

// Case sensitivity
console.log("a vs A:", "a".localeCompare("A")); // locale-dependent, typically -1 or 1
// The important thing is that same strings return 0
console.log("same string:", "hello".localeCompare("hello") === 0); // true

// Accented characters
console.log("a vs a-acute sign:", "a".localeCompare("\u00E1")); // negative (a before a-acute in most locales)

// --- Additional edge cases ---

// str.at with unicode
const unicodeStr = "cafe\u0301"; // cafe with combining accent on e
console.log("unicode at(3):", unicodeStr.at(3)); // 'e' (the base character)
console.log("unicode at(4):", unicodeStr.at(4)); // combining accent character

// String.fromCodePoint with zero
console.log("fromCodePoint(0):", String.fromCodePoint(0) === "\0"); // true

// Empty String.raw
const rawEmpty = String.raw``;
console.log("String.raw empty:", rawEmpty); // ''
console.log("String.raw empty length:", rawEmpty.length); // 0

console.log("All string gap tests complete.");
