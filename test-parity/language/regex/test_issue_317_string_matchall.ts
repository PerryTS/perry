// Regression for #317: String.prototype.matchAll(regex) used to fail at
// codegen with "perry-codegen Phase 2: expression StringMatchAll not yet
// supported". HIR + runtime helper already existed; only the codegen arm
// and runtime_decls extern were missing. Surfaced compiling
// effect/src/internal/cause.ts during the #309 compat sweep.

// Issue's literal repro: `for...of` over matchAll, accessing capture
// groups by index. The runtime fix also NaN-boxes inner array pointers
// so `m[1]` / `m[2]` read back correctly through the IndexGet path
// (without the fix the slot held raw pointer bits and `m[1]` returned a
// nonsense double).
const text = "key=val&foo=bar&baz=qux";
const re = /([a-z]+)=([a-z]+)/g;
for (const m of text.matchAll(re)) {
  console.log(m[1], "->", m[2]);
}

// Full match at index 0 + named indices.
const text2 = "2026-04-30 and 2026-05-01";
const dateRe = /(\d{4})-(\d{2})-(\d{2})/g;
for (const m of text2.matchAll(dateRe)) {
  console.log(m[0], m[1], m[2], m[3]);
}

// No matches: matchAll returns empty iterable, never null.
const noMatch = "hello world";
const digits = /\d+/g;
let count = 0;
for (const _ of noMatch.matchAll(digits)) count++;
console.log("noMatch count:", count);

// Spread into Array — each entry is itself an array of (full, ...groups).
const text3 = "aaa bbb ccc";
const wordRe = /(\w)\w+/g;
const arr = [...text3.matchAll(wordRe)];
console.log("len:", arr.length);
console.log(arr[0][0], arr[0][1]);
console.log(arr[1][0], arr[1][1]);
console.log(arr[2][0], arr[2][1]);

// Single match.
const text4 = "only one";
const oneRe = /one/g;
for (const m of text4.matchAll(oneRe)) {
  console.log("single:", m[0]);
}
