// Coverage for the string-concat perf fix (perry-runtime/src/string/concat.rs
// + string/mod.rs): the ASCII-ness of a concat operand is now read off the
// StringHeader (`utf16_len == byte_len`) instead of re-scanning bytes, the
// short-concat memo probe hashes/looks-up two operand slices directly
// instead of assembling them into a scratch buffer first, and the memo
// governor's minimum-hit-rate floor changed. None of that may change any
// observable result: every case here is compared byte-for-byte against
// `node --experimental-strip-types`.
//
// Values are built from runtime state (array/loop indices), never folded to
// a compile-time constant, so codegen must actually reach the runtime concat
// paths under test.

function codes(s: string): string {
  let out = "";
  for (let i = 0; i < s.length; i++) out += (i ? "+" : "") + s.charCodeAt(i).toString(16);
  return out;
}

// ---------------------------------------------------------------------------
// 1. ASCII string+string concat across the SSO (5) and memo (12) byte
//    ceilings — exercises concat_byte_parts's SSO fast path, memo path, and
//    heap path in one sweep.
// ---------------------------------------------------------------------------
const lensA = [0, 0, 2, 3, 6, 6, 10, 36];
const lensB = [0, 1, 3, 3, 6, 7, 10, 37];
for (let i = 0; i < lensA.length; i++) {
  const a = "x".repeat(lensA[i]);
  const b = "y".repeat(lensB[i]);
  const r = a + b;
  console.log("ss", lensA[i], lensB[i], r.length, r);
}

// Same boundary set, but string+number (js_string_concat_value /
// js_value_concat_string) and number+string, so the "prefix" + i arm's
// memoizable gate (also touched by this fix) is covered too.
const prefixLens = [0, 1, 4, 5, 6, 11, 12, 13, 20];
for (let i = 0; i < prefixLens.length; i++) {
  const prefix = "p".repeat(prefixLens[i]);
  const withNum = prefix + i;
  const numWith = i + prefix;
  console.log("sn", prefixLens[i], withNum.length, withNum, numWith.length, numWith);
}

// ---------------------------------------------------------------------------
// 2. Non-ASCII, valid (well-formed) UTF-16 — 2-byte, 3-byte and 4-byte
//    (astral, via a literal, not a joined surrogate pair) UTF-8 operands.
//    These print directly: valid Unicode encodes identically to UTF-8 in
//    both engines, so a byte-for-byte diff is a meaningful check on its own.
// ---------------------------------------------------------------------------
const twoByte = "é"; // U+00E9, 2 UTF-8 bytes, 1 UTF-16 unit
const threeByte = "€"; // U+20AC, 3 UTF-8 bytes, 1 UTF-16 unit
const fourByte = "😀"; // U+1F600, 4 UTF-8 bytes, 2 UTF-16 units (already a pair)
const nonAsciiCases: [string, string][] = [
  ["2b+ascii", twoByte + "ab"],
  ["ascii+2b", "ab" + twoByte],
  ["2b+2b", twoByte + twoByte],
  ["3b+ascii", threeByte + "ab"],
  ["ascii+3b", "ab" + threeByte],
  ["3b+3b", threeByte + threeByte],
  ["4b+ascii", fourByte + "ab"],
  ["ascii+4b", "ab" + fourByte],
  ["4b+4b", fourByte + fourByte],
  ["mixed", "a" + twoByte + "b" + threeByte + "c" + fourByte + "d"],
];
for (const [name, s] of nonAsciiCases) {
  console.log("na", name, s.length, s, s.isWellFormed());
}

// string+number and number+string with a non-ASCII prefix/suffix, to hit
// js_string_concat_value / js_value_concat_string's non-ASCII path.
for (let i = 0; i < 3; i++) {
  const withNum = threeByte + i;
  const numWith = i + fourByte;
  console.log("nan", i, withNum.length, withNum, numWith.length, numWith);
}

// ---------------------------------------------------------------------------
// 3. Lone surrogates and a surrogate pair formed ACROSS the join boundary.
//    Raw lone-surrogate content is reported via charCodeAt (codes()) or
//    JSON.stringify — both are byte-safe (JSON.stringify escapes an
//    unpaired surrogate as \uXXXX rather than emitting it raw), matching
//    the pattern used elsewhere in this suite (#9431). A properly merged
//    astral pair is well-formed Unicode and is printed directly.
// ---------------------------------------------------------------------------
const hi = "\uD83D"; // lone high surrogate
const lo = "\uDE00"; // lone low surrogate — hi+lo is exactly 😀 (U+1F600)

// Pair formed directly across the join boundary: canonicalize_surrogate_pairs
// must merge it, so this is well-formed and safe to print raw.
const paired = hi + lo;
console.log("pair-direct", paired.length, paired, paired.isWellFormed(), paired.codePointAt(0));

// Reverse order never forms a pair (low-before-high is not a valid pair) —
// must remain two lone surrogates.
const reversed = lo + hi;
console.log(
  "pair-reversed",
  reversed.length,
  codes(reversed),
  reversed.isWellFormed(),
  JSON.stringify(reversed),
);

// A pair split across TWO separate concatenations, then joined by a THIRD:
// "a" + hi built first, lo + "b" built second, then those two results
// concatenated — the pair only becomes adjacent at the last join.
const left = "a" + hi;
const right = lo + "b";
const rejoined = left + right;
console.log(
  "pair-split-rejoin",
  left.length,
  right.length,
  rejoined.length,
  rejoined,
  rejoined.isWellFormed(),
  rejoined.codePointAt(1),
);

// A lone surrogate with ASCII on both sides never forms a pair — stays lone,
// flag preserved through the concat.
const loneMid = "x" + hi + "y";
console.log("lone-mid", loneMid.length, codes(loneMid), loneMid.isWellFormed(), JSON.stringify(loneMid));

// Two highs in a row: no valid pair (high+high is not low-after-high).
const twoHighs = hi + hi;
console.log("two-highs", twoHighs.length, codes(twoHighs), twoHighs.isWellFormed());

// ---------------------------------------------------------------------------
// 4. Empty operands on both sides of both concat forms.
// ---------------------------------------------------------------------------
console.log("empty-both", ("" + "").length, JSON.stringify("" + ""));
console.log("empty-left", ("" + "z").length, "" + "z");
console.log("empty-right", ("z" + "").length, "z" + "");
console.log("empty-num", ("" + 0).length, "" + 0, (0 + "").length, 0 + "");

// ---------------------------------------------------------------------------
// 5. Repeated identical concat results — forces the memo doorkeeper's
//    "seen twice" admission and then real hits, and checks `===` identity
//    across independently-built equal results (the memo must never change
//    observable semantics: value equality is unaffected either way, but a
//    hash-collision or admission bug would surface as a wrong `.length` or
//    a `false` here).
// ---------------------------------------------------------------------------
let memoFailures = 0;
const memoResults: string[] = [];
for (let i = 0; i < 40; i++) {
  // Same content, two different operand splits — "ab" + "cdef" and
  // "abc" + "def" both yield "abcdef".
  const viaSplitA = "ab" + "cdef".slice(0);
  const viaSplitB = "abc".slice(0) + "def";
  if (viaSplitA !== viaSplitB) memoFailures++;
  if (viaSplitA.length !== 6) memoFailures++;
  memoResults.push(viaSplitA);
}
for (let i = 1; i < memoResults.length; i++) {
  if (memoResults[i] !== memoResults[0]) memoFailures++;
}
console.log("memo-repeat-failures", memoFailures, memoResults.length, memoResults[0]);

// A heap-forced (>SSO, <=memo-ceiling) equal pair built two different ways,
// repeated enough to admit, then compared for identity and content.
let memoHeapFailures = 0;
for (let i = 0; i < 40; i++) {
  const a = "field_" + "ab".slice(0); // "field_ab", 8 bytes
  const b = "field" + "_ab".slice(0);
  if (a !== b || a.length !== 8 || a !== "field_ab") memoHeapFailures++;
}
console.log("memo-heap-repeat-failures", memoHeapFailures);

// The "prefix" + i shape repeated with a REPEATED i, so the SAME result
// recurs (as opposed to section 1's sweep, which never repeats a value).
let memoNumFailures = 0;
const memoNumResults: string[] = [];
for (let rep = 0; rep < 30; rep++) {
  const k = "row_" + 7;
  if (k.length !== 5 || k !== "row_7") memoNumFailures++;
  memoNumResults.push(k);
}
for (let i = 1; i < memoNumResults.length; i++) {
  if (memoNumResults[i] !== memoNumResults[0]) memoNumFailures++;
}
console.log("memo-num-repeat-failures", memoNumFailures, memoNumResults[0]);

console.log("done");
