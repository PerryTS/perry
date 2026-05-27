// Issue #40 / #1862 / #321 regression: effect/Schema `decodeUnknownSync`
// SIGSEGV'd on `main` after the #1862 "raw f64 array slot canonicalization"
// landed. Root cause: Perry NaN-boxes a class reference with INT32_TAG
// (0x7FFE) — the SAME bit shape as a genuine small integer — and keys class
// dispatch off that tag. The numeric-array canonicalization rewrote any
// int32-shaped slot of a RawF64 array to its raw f64 bits, stripping the
// 0x7FFE tag from class refs that flowed through a numeric-seeded array. A
// later property read then missed the class-ref dispatch arm and
// dereferenced the raw double as a heap pointer (crash in
// `is_registered_set` via `js_array_map` -> `js_object_get_field_by_name`).
//
// The fix makes the numeric-layout predicate reject int32-shaped values that
// are registered class refs, so storing a class ref into a numeric array
// downgrades the layout and preserves the NaN-box bits — genuine integers
// still canonicalize (the #1862 feature is intact).
//
// Expected output:
// T,B,n,n,n,n,n,n,n,n
// 42 cfg 7 oth

class Tag {
  static k = "T";
}
class Box {
  static k = "B";
}

// Erase the static class-ref knowledge so the value flows dynamically (the
// way effect/Schema passes schema "constructors" through generic arrays).
function wrap(x: any): any {
  return x;
}

// Seed a RawF64 numeric layout, then overwrite slots with INT32-tagged class
// refs, then read them back via `.map` (which reads each slot raw) + a
// property access in the callback.
const items: any[] = [];
for (let i = 0; i < 10; i++) items.push(i);
items[0] = wrap(Tag);
items[1] = wrap(Box);
const ids = items.map((it: any) => (typeof it === "function" && it.k ? it.k : "n"));
console.log(ids.join(","));

// Direct read-back of a class ref stored in a numeric-seeded array, then a
// static field access (must not be corrupted by canonicalization).
class Config {
  static version = 42;
  static label = "cfg";
}
class Other {
  static version = 7;
  static label = "oth";
}
const slots: any[] = [];
for (let i = 0; i < 8; i++) slots.push(i);
slots[3] = Config;
slots[5] = Other;
const c = slots[3];
const o = slots[5];
console.log(c.version, c.label, o.version, o.label);
