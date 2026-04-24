// Tests for String.prototype.isWellFormed() and toWellFormed() (ES2024)
// Covers: lone high surrogate, lone low surrogate, paired surrogates,
// ASCII+surrogate mix, well-formed strings.

// Well-formed strings
console.log("Hello World".isWellFormed());       // true
console.log("".isWellFormed());                  // true
console.log("𐀀".isWellFormed());                 // true (U+10000 paired surrogates)
console.log("café".isWellFormed());              // true

// Lone high surrogate
console.log("\uD800".isWellFormed());            // false
console.log("\uDBFF".isWellFormed());            // false

// Lone low surrogate
console.log("\uDC00".isWellFormed());            // false
console.log("\uDFFF".isWellFormed());            // false

// Mixed: ASCII + lone surrogate
console.log("ab\uD800cd".isWellFormed());        // false
console.log("ab\uD800cd".length);               // 5

// toWellFormed replaces lone surrogates with U+FFFD
const a = "\uD800".toWellFormed();
console.log(a.isWellFormed());                   // true
console.log(a === "�");                     // true
console.log(a.length);                           // 1

const b = "ab\uD800cd".toWellFormed();
console.log(b.isWellFormed());                   // true
console.log(b.includes("�"));              // true
console.log(b.length);                           // 5

// toWellFormed on well-formed string is identity
const c = "Hello".toWellFormed();
console.log(c === "Hello");                      // true
console.log(c.isWellFormed());                   // true

// Paired surrogate (𐀀 = U+10000) is well-formed — toWellFormed preserves it
const d = "𐀀".toWellFormed();
console.log(d === "𐀀");                          // true
console.log(d.length);                           // 2
