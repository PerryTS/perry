// #7621: every `path.*` codegen arm that hands its operand to the runtime as a
// raw `*const StringHeader` read an SSO (small-string-optimized) string's
// INLINE BYTES as a pointer.
//
// A NaN-boxed string is either a heap `StringHeader*` (STRING_TAG = 0x7FFF) or,
// for <= 5 bytes, the characters themselves packed into the payload
// (SHORT_STRING_TAG = 0x7FF9). The arms unboxed with `unbox_to_i64`, which just
// masks the low 48 bits: correct for the heap form, and for the inline form it
// is the CHARACTERS, dereferenced as a header. `path.resolve("/root", seg(1))`
// therefore threw `TypeError [ERR_INVALID_ARG_TYPE]` while the literal form
// worked — literals are interned to the heap, so only a COMPUTED short string
// takes the inline representation.
//
// Everything here is computed at runtime from `n` so no constant folding can
// turn a probe back into a literal, and every operand family is swept ACROSS
// the 5-byte SSO boundary rather than at one length, because that boundary is
// where the two representations swap.

import * as path from "node:path";

// Runtime-computed segments. `n` comes from an array index the compiler cannot
// fold, so `seg(1)` is a real concatenation producing a 2-byte SSO string and
// `seg(20)` a 21-byte heap string.
const ns: number[] = [1, 2, 3, 4, 5, 6, 7, 8, 20];
function seg(n: number): string {
  return "s" + "e".repeat(n);
}
function computed(n: number): string {
  return ns[n] === undefined ? "?" : "s" + String(ns[n]);
}

// ── 1. the issue's exact repro: literal vs computed, same value ──
console.log("1 literal :", path.resolve("/root", "s1"));
console.log("1 computed:", path.resolve("/root", computed(0)));
console.log("1 equal   :", path.resolve("/root", "s1") === path.resolve("/root", computed(0)));

// ── 2. sweep the SSO boundary (inline holds <= 5 bytes) ──
for (let n = 1; n <= 9; n++) {
  const s = seg(n);
  console.log(`2 len=${s.length}`, path.resolve("/root", s));
}

// ── 3. multi-segment resolve, computed in the middle and at the end ──
console.log("3 three:", path.resolve("/a", computed(2), "d"));
console.log("3 four :", path.resolve("/a", "b", computed(3), computed(4)));
console.log("3 reset:", path.resolve("/a", computed(0), "/abs", computed(1)));
console.log("3 dots :", path.resolve("/a/b/c", computed(0), "..", computed(1)));

// ── 4. relative base — cwd-dependent, so assert the SHAPE, not the bytes ──
const rel = path.resolve("rel", computed(0));
console.log("4 rel suffix:", rel.endsWith("/rel/s1"), rel.startsWith("/"));
console.log("4 rel equal :", rel === path.resolve(process.cwd(), "rel", computed(0)));
const relEmpty = path.resolve(computed(0));
console.log("4 bare      :", relEmpty.endsWith("/s1"), relEmpty === path.resolve(process.cwd(), "s1"));

// ── 5. the sibling arms that share the raw-pointer unbox ──
console.log("5 join     :", path.join("/root", computed(0)), path.join(computed(0), computed(1)));
console.log("5 normalize:", path.normalize(computed(0)), path.normalize("/a/" + computed(0) + "/../b"));
console.log("5 extname  :", JSON.stringify(path.extname(computed(0) + ".ts")), JSON.stringify(path.extname(computed(0))));
console.log("5 dirname  :", path.dirname("/a/" + computed(0)), path.dirname(computed(0)));
console.log("5 basename :", path.basename("/a/" + computed(0)), path.basename(computed(0)));
console.log("5 basenExt :", path.basename(computed(0) + ".ts", ".ts"), path.basename("a.ts", ".ts"));
console.log("5 isAbs    :", path.isAbsolute(computed(0)), path.isAbsolute("/" + computed(0)));
console.log("5 relative :", path.relative("/a/" + computed(0), "/a/" + computed(1)));
console.log("5 win32join:", path.win32.join("C:\\r", computed(0)));

const parsed = path.parse("/a/" + computed(0) + ".ts");
console.log("5 parse    :", parsed.root, parsed.dir, parsed.base, parsed.ext, parsed.name);

console.log("5 glob     :", path.matchesGlob(computed(0) + ".ts", "*.ts"), path.matchesGlob(computed(0), "*.ts"));

// ── 6. non-string operands must still throw ERR_INVALID_ARG_TYPE ──
try {
  // deno-lint-ignore no-explicit-any
  path.resolve("/root", ns as any);
  console.log("6 resolve non-string: NO THROW");
} catch (e) {
  console.log("6 resolve non-string:", (e as Error).name, (e as { code?: string }).code);
}
try {
  // deno-lint-ignore no-explicit-any
  path.join("/root", 5 as any);
  console.log("6 join non-string: NO THROW");
} catch (e) {
  console.log("6 join non-string:", (e as Error).name, (e as { code?: string }).code);
}

// ── 7. the SSO operand must survive an allocation between the two unboxes ──
// `path.resolve(a, b)` folds to `PathResolveJoin(a, b)`, and materializing an
// inline `a` onto the heap is itself an allocation — so `b` has to be re-read
// after it, not held in a register across it. Churn the nursery first so a
// collection actually lands inside that window.
function churn(): string {
  let last = "";
  for (let i = 0; i < 2000; i++) {
    last = { k: "v" + String(i) }.k;
  }
  return last;
}
churn();
let acc = "";
for (let i = 0; i < 200; i++) {
  acc = path.resolve("/r" + String(i % 3), computed(i % 5), computed((i + 1) % 5));
  churn();
}
console.log("7 window:", acc);
