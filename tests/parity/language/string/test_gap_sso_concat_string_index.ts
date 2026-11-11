// A short string built by concatenation is an inline SHORT_STRING_TAG (SSO)
// JSValue whose payload IS the characters, not a heap address. Codegen's `s[i]`
// fast path mask-unboxed the receiver to a `StringHeader*`, and
// `js_array_from_value` did the same mask itself, so both produced a bogus
// pointer and segfaulted. `"ab" + "c"` is the ordinary way to make one, which
// made `(a + b)[0]`, `for (const ch of a + b)` and `Array.from(a + b)` crash
// while the identical operations on a literal or a `join()` result were fine.
//
// Long concatenations exceed the SSO threshold and were always heap-backed —
// they are covered here so the fix cannot regress the heap path.

const a = "ab";
const b = "c";
const short = a + b;

console.log(typeof short, short.length, short);

// --- indexed reads on a short concatenation -------------------------------
console.log(short[0], short[1], short[2]);
console.log(String(short[3]));           // undefined, out of range
console.log(String(short[-1]));          // undefined, negative
console.log(short.charAt(1), short.charCodeAt(1), short.codePointAt(1));
console.log(short.at(0), short.at(-1));

// index-loop accumulation (the shape milo's codegen uses)
let viaIndex = "";
for (let i = 0; i < short.length; i++) viaIndex += short[i];
console.log(viaIndex);

// --- iteration protocols --------------------------------------------------
let viaForOf = 0;
for (const ch of short) viaForOf++;
console.log(viaForOf);
console.log([...short].join("-"));
console.log(Array.from(short).length, Array.from(short).join("|"));
console.log(short.split("").join("+"));

// --- concatenation of a join result, exactly milo's shape -----------------
const parts: string[] = [];
parts.push("%.*s");
const fmt = parts.join("") + "\n";
console.log(fmt.length);
let fmtChars = 0;
for (const ch of fmt) fmtChars++;
console.log(fmtChars);
console.log(fmt[0], fmt[1], JSON.stringify(fmt[4]));

// --- empty and single-char concatenations ---------------------------------
const empty = "" + "";
console.log(empty.length, String(empty[0]), Array.from(empty).length);
const one = "" + "x";
console.log(one.length, one[0], Array.from(one).length);

// --- non-ASCII, where UTF-16 indexing differs from bytes ------------------
const uni = "é" + "ü";
console.log(uni.length, uni[0], uni[1], Array.from(uni).length);

// --- long (heap-backed) concatenation must still work ---------------------
const long = "a".repeat(40) + "b".repeat(40);
console.log(long.length, long[0], long[79], Array.from(long).length);

// --- concatenation built in a loop ----------------------------------------
let acc = "";
for (const p of ["x", "y", "z"]) acc += p;
console.log(acc.length, acc[0], acc[2], Array.from(acc).join(""));
