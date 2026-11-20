// Regression for #315: String.prototype.startsWith / endsWith 2-arg form
// (searchString, position) used to fail at codegen with
// "perry-codegen: String.startsWith expects 1 arg, got 2".
// Surfaced compiling effect/src/String.ts during the #309 compat sweep.

const s = "abcdef";

// startsWith(searchString, position) — 2-arg form.
console.log(s.startsWith("cd", 2));   // true
console.log(s.startsWith("ab", 0));   // true
console.log(s.startsWith("ab"));      // true (1-arg form still works)
console.log(s.startsWith("cd", 0));   // false
console.log(s.startsWith("ef", 4));   // true

// Position clamping: negative -> 0, beyond length -> length.
console.log(s.startsWith("ab", -5));  // true
console.log(s.startsWith("", 100));   // true (empty prefix at clamped end)

// endsWith(searchString, endPosition) — truncate to endPosition then check.
console.log(s.endsWith("cd", 4));     // true
console.log(s.endsWith("ef"));        // true (1-arg)
console.log(s.endsWith("ef", 6));     // true
console.log(s.endsWith("ab", 2));     // true
console.log(s.endsWith("ef", 4));     // false (truncated to "abcd")

// endPosition clamping.
console.log(s.endsWith("ab", -1));    // false (clamped to 0; only "" matches at 0)
console.log(s.endsWith("", 0));       // true
console.log(s.endsWith("ef", 100));   // true (clamped to length)

// Multi-byte UTF-8 / UTF-16 indexing.
const u = "αβγδε";
console.log(u.startsWith("β", 1));    // true
console.log(u.startsWith("γ", 2));    // true
console.log(u.endsWith("δε", 5));     // true
console.log(u.endsWith("βγ", 3));     // true
