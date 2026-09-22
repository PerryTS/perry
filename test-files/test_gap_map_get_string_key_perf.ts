// Gap test: Map.get(string) correctness under the fast string-keyed lookup
// path (content-hash side table + SameValueZero identity rules). Covers the
// shapes exercised while reducing Map.get(str) instruction cost: identity
// semantics (NaN, +0/-0), insertion-order iteration, has-vs-get with a
// stored `undefined`, delete-then-reinsert order, non-ASCII / lone-surrogate
// string keys, content-equal-but-distinct-identity string keys, and a map
// past the side-table growth threshold.
// Run: node --experimental-strip-types test_gap_map_get_string_key_perf.ts

// --- SameValueZero: NaN keys collapse to one slot ---
const nanMap = new Map<number, string>();
nanMap.set(NaN, "first");
nanMap.set(NaN, "second");
console.log("nan size:", nanMap.size);
console.log("nan get:", nanMap.get(NaN));
console.log("nan has:", nanMap.has(NaN));

// --- SameValueZero: +0 and -0 are the same key ---
const zeroMap = new Map<number, string>();
zeroMap.set(0, "plus");
zeroMap.set(-0, "minus");
console.log("zero size:", zeroMap.size);
console.log("zero get +0:", zeroMap.get(0));
console.log("zero get -0:", zeroMap.get(-0));

// --- has() vs get() for a stored `undefined` value ---
const undefMap = new Map<string, number | undefined>();
undefMap.set("present", undefined);
console.log("undef has present:", undefMap.has("present"));
console.log("undef get present:", undefMap.get("present"));
console.log("undef has missing:", undefMap.has("missing"));
console.log("undef get missing:", undefMap.get("missing"));

// --- Insertion-order iteration survives get()-only probing ---
const orderMap = new Map<string, number>();
orderMap.set("z", 1);
orderMap.set("a", 2);
orderMap.set("m", 3);
orderMap.get("a");
orderMap.get("z");
const orderKeys: string[] = [];
for (const k of orderMap.keys()) orderKeys.push(k);
console.log("order keys:", orderKeys);

// --- delete() then re-insert moves a key to the end ---
const reinsMap = new Map<string, number>();
reinsMap.set("x", 1);
reinsMap.set("y", 2);
reinsMap.set("z", 3);
reinsMap.delete("x");
reinsMap.set("x", 10);
const reinsKeys: string[] = [];
for (const k of reinsMap.keys()) reinsKeys.push(k);
console.log("reins keys:", reinsKeys);
console.log("reins get x:", reinsMap.get("x"));

// --- Non-ASCII string keys ---
const uniMap = new Map<string, string>();
uniMap.set("héllo", "accent");
uniMap.set("日本語", "japanese");
uniMap.set("😀emoji", "emoji");
console.log("uni get héllo:", uniMap.get("héllo"));
console.log("uni get 日本語:", uniMap.get("日本語"));
console.log("uni get emoji:", uniMap.get("😀emoji"));
console.log("uni get missing:", uniMap.get("héllo2"));

// --- Lone-surrogate string keys (WTF-8) ---
const loneHigh = String.fromCharCode(0xd800);
const loneLow = String.fromCharCode(0xdc00);
const surrMap = new Map<string, string>();
surrMap.set(loneHigh, "high");
surrMap.set(loneLow, "low");
console.log("surr get high:", surrMap.get(loneHigh));
console.log("surr get low:", surrMap.get(loneLow));
console.log("surr high !== low:", loneHigh !== loneLow);
console.log(
  "surr get rebuilt high:",
  surrMap.get(String.fromCharCode(0xd800)),
);

// --- Content-equal but distinct-identity keys (dynamically built) ---
function makeKey(prefix: string, n: number): string {
  return prefix + n;
}
const dynMap = new Map<string, number>();
for (let i = 0; i < 20; i++) {
  dynMap.set(makeKey("item", i), i * 10);
}
// Re-build the same content via a different allocation than the stored key.
console.log("dyn get item0 (rebuilt):", dynMap.get(makeKey("item", 0)));
console.log("dyn get item19 (rebuilt):", dynMap.get(makeKey("item", 19)));
console.log("dyn get item9 (rebuilt):", dynMap.get(makeKey("item", 9)));
console.log("dyn get missing:", dynMap.get(makeKey("item", 99)));

// --- Map past the side-table growth threshold (small linear scan vs
// hashed side table) ---
const bigMap = new Map<string, number>();
for (let i = 0; i < 64; i++) {
  bigMap.set("k" + i, i);
}
console.log("big size:", bigMap.size);
console.log("big get k0:", bigMap.get("k0"));
console.log("big get k63:", bigMap.get("k63"));
console.log("big get k32 (rebuilt):", bigMap.get(makeKey("k", 32)));
console.log("big get missing:", bigMap.get("nomatch"));
bigMap.delete("k10");
bigMap.delete("k20");
bigMap.set("k10", 1010);
console.log("big get k10 after delete+reinsert:", bigMap.get("k10"));
console.log("big has k20 after delete:", bigMap.has("k20"));
console.log("big size after churn:", bigMap.size);

// --- Long (> 64 byte) string keys ---
const longMap = new Map<string, number>();
const longPrefix = "q".repeat(70);
for (let i = 0; i < 10; i++) {
  longMap.set(longPrefix + i, i);
}
console.log("long get 0 (rebuilt):", longMap.get(longPrefix + 0));
console.log("long get 9 (rebuilt):", longMap.get(longPrefix + 9));
console.log("long get missing:", longMap.get(longPrefix + "zz"));

// --- Interned literal keys get a pointer-equality shortcut but must
// still match content-equal keys built at runtime ---
const litMap = new Map<string, number>();
litMap.set("literal-key", 1);
console.log("lit get literal:", litMap.get("literal-key"));
console.log(
  "lit get rebuilt:",
  litMap.get(["li", "teral-key"].join("")),
);
