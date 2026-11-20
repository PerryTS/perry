// #7570: a `class X extends Map | Set` instance held in a binding ANNOTATED
// with the BASE type was handed to the raw `js_map_*` / `js_set_*` entry points
// as if it were a real `MapHeader`/`SetHeader`.
//
// Perry models a Map/Set subclass instance as a plain `ObjectHeader`, and the
// two headers overlay field-for-field:
//
//   MapHeader.size     (+0) ← ObjectHeader.object_type
//   MapHeader.capacity (+4) ← ObjectHeader.class_id
//   MapHeader.entries  (+8) ← parent_class_id ‖ field_count   ← a FORGED POINTER
//
// so the first `.set()` stored through two glued-together `u32` class ids and
// the process took SIGBUS before printing anything.
//
// The cause is that "is a Map" was decided from the DECLARED TypeScript type
// (`is_map_expr` ⇐ `Type::Generic { base: "Map" }`), and a declared type is a
// hint, never a layout fact — nothing validates annotations at runtime. Every
// binding form that can carry the base type is therefore a way in; each one
// below crashed before the fix. The UNANNOTATED shape (covered by
// test_gap_6325_map_set_subclass.ts) always worked, because it dispatches
// through the hidden collection backing.
//
// Fixed by resolving the receiver at the raw runtime entry points: a genuine
// header passes straight through, a subclass instance is redirected onto its
// backing, and a plain object merely annotated `Map<K, V>` degrades to the
// existing null handling instead of dereferencing a forged pointer.

class MyMap<K, V> extends Map<K, V> {}
class MySet<T> extends Set<T> {}

// ── 1. `const` annotated with the base type — the issue's exact repro ──
const m1: Map<string, number> = new MyMap<string, number>();
m1.set("a", 1);
console.log("1 const-ann:", m1.get("a"), m1.size, m1.has("a"), m1.has("zz"));
console.log("1 delete:", m1.delete("zz"), m1.delete("a"), m1.size);

const s1: Set<number> = new MySet<number>();
s1.add(4);
s1.add(4);
console.log("1 set const-ann:", s1.size, s1.has(4), s1.has(5), s1.delete(4), s1.size);

// ── 2. PARAMETER annotated with the base type ──
function takeMap(mm: Map<string, number>): string {
  let total = 0;
  for (const [, v] of mm) total += v;
  return `${total} ${mm.size} ${mm.get("x")}`;
}
console.log(
  "2 param-ann:",
  takeMap(
    new MyMap<string, number>([
      ["x", 5],
      ["y", 6],
    ]),
  ),
);

function takeSet(ss: Set<number>): string {
  let total = 0;
  for (const v of ss) total += v;
  return `${total} ${ss.size} ${ss.has(2)}`;
}
console.log("2 set param-ann:", takeSet(new MySet<number>([1, 2, 3])));

// ── 3. CLASS FIELD annotated with the base type ──
class Holder {
  m: Map<string, number> = new MyMap<string, number>();
  s: Set<number> = new MySet<number>();
  run(): string {
    this.m.set("k", 7);
    this.s.add(9);
    return `${this.m.get("k")} ${this.m.size} ${this.s.has(9)} ${this.s.size}`;
  }
}
console.log("3 field-ann:", new Holder().run());

// ── 4. RETURN TYPE annotated with the base type ──
function makeMap(): Map<string, number> {
  return new MyMap<string, number>([["r", 3]]);
}
const m4 = makeMap();
m4.set("q", 4);
console.log("4 ret-ann:", m4.get("r"), m4.get("q"), m4.size);

function makeSet(): Set<number> {
  return new MySet<number>([7]);
}
const s4 = makeSet();
s4.add(8);
console.log("4 set ret-ann:", s4.size, s4.has(7), s4.has(8));

// ── 5. `as` CAST to the base type ──
const m5 = new MyMap<string, number>() as Map<string, number>;
m5.set("c", 8);
console.log("5 as-cast:", m5.get("c"), m5.size);

const s5 = new MySet<number>() as Set<number>;
s5.add(11);
console.log("5 set as-cast:", s5.size, s5.has(11));

// ── 6. iteration surface through an annotated binding ──
const m6: Map<string, number> = new MyMap<string, number>([
  ["a", 1],
  ["b", 2],
]);
console.log("6 values:", [...m6.values()].join(","));
console.log("6 keys:", [...m6.keys()].join(","));
console.log("6 entries:", JSON.stringify([...m6.entries()]));
console.log("6 spread:", JSON.stringify([...m6]));
console.log("6 Array.from:", JSON.stringify(Array.from(m6.keys())));
let acc6 = 0;
// The 3rd callback argument is the RECEIVER, not the hidden backing collection.
let sameMap6 = true;
m6.forEach((v, k, self) => {
  acc6 += v + k.length;
  if (self !== m6) sameMap6 = false;
});
console.log("6 forEach:", acc6, sameMap6);
const seen6: string[] = [];
for (const [k, v] of m6) seen6.push(`${k}=${v}`);
console.log("6 for-of:", seen6.join("|"));

const s6: Set<number> = new MySet<number>([1, 2, 3]);
console.log("6 set values:", [...s6.values()].join(","));
console.log("6 set keys:", [...s6.keys()].join(","));
console.log("6 set spread:", [...s6].join(","));
let acc6s = 0;
let sameSet6 = true;
s6.forEach((v, _k, self) => {
  acc6s += v;
  if (self !== s6) sameSet6 = false;
});
console.log("6 set forEach:", acc6s, sameSet6);
const seen6s: number[] = [];
for (const v of s6) seen6s.push(v);
console.log("6 set for-of:", seen6s.join(","));

// ── 7. RECEIVER IDENTITY: `.set()`/`.add()` return the INSTANCE, not the
//       hidden backing collection. Chaining and `===` must both hold. ──
const m7: Map<string, number> = new MyMap<string, number>();
console.log("7 set returns receiver:", m7.set("a", 1) === m7);
m7.set("b", 2).set("c", 3);
console.log("7 chained:", m7.size, m7.get("b"), m7.get("c"));
// The SUBCLASS edge is asserted too since #7575: `MyMap<K, V>` is generic, so
// `new MyMap<string, number>()` constructs the monomorphized `MyMap$str_num`
// and `instanceof MyMap` used to miss the class the user actually wrote.
console.log("7 still a Map:", m7 instanceof Map, m7 instanceof MyMap);

const s7: Set<number> = new MySet<number>();
console.log("7 add returns receiver:", s7.add(1) === s7);
s7.add(2).add(3);
console.log("7 set chained:", s7.size, s7.has(2), s7.has(3));
console.log("7 still a Set:", s7 instanceof Set, s7 instanceof MySet);

// ── 8. `clear()` through an annotated binding ──
const m8: Map<string, number> = new MyMap<string, number>([["z", 1]]);
m8.clear();
console.log("8 clear:", m8.size, m8.get("z"));

const s8: Set<number> = new MySet<number>([1, 2]);
s8.clear();
console.log("8 set clear:", s8.size, s8.has(1));

// ── 9. INDIRECT subclass and a subclass with its own ctor + fields, both
//       reached through a base-typed binding ──
class MidMap extends Map<string, number> {}
class LeafMap extends MidMap {}
const m9: Map<string, number> = new LeafMap();
m9.set("deep", 42);
console.log("9 indirect:", m9.get("deep"), m9.size);

class TaggedMap extends Map<string, number> {
  tag: string;
  constructor(tag: string) {
    super([["seed", 1]]);
    this.tag = tag;
  }
}
const tagged = new TaggedMap("t");
const m9b: Map<string, number> = tagged;
m9b.set("more", 2);
console.log("9 own ctor:", m9b.get("seed"), m9b.get("more"), m9b.size, tagged.tag);

// ── 10. NON-SUBCLASS CONTROLS. These take the raw fast path and must be
//       byte-identical to before the fix. ──
const plain: Map<string, number> = new Map<string, number>([["p", 1]]);
plain.set("p2", 2);
console.log("10 plain:", plain.get("p"), plain.size, [...plain.keys()].join(","));
console.log("10 plain identity:", plain.set("p3", 3) === plain, plain.size);
let plainSelf = true;
plain.forEach((_v, _k, self) => {
  if (self !== plain) plainSelf = false;
});
console.log("10 plain forEach self:", plainSelf);
const plainSet: Set<number> = new Set<number>([1, 2]);
plainSet.add(3);
console.log("10 plainSet:", plainSet.size, plainSet.has(3), [...plainSet].join(","));
console.log("10 plainSet identity:", plainSet.add(4) === plainSet, plainSet.size);

// A number-keyed Map and a string→string Map exercise the SPECIALIZED
// `js_map_set_*` entry points (the crash frame in the issue was
// `map_set_string_key_value`).
const nums: Map<number, number> = new Map<number, number>();
for (let i = 0; i < 4; i++) nums.set(i, i * i);
console.log("10 numeric:", nums.get(3), nums.size);
const strs: Map<string, string> = new Map<string, string>();
strs.set("k", "v");
console.log("10 string-string:", strs.get("k"), strs.size);

const numsSub: Map<number, number> = new MyMap<number, number>();
for (let i = 0; i < 4; i++) numsSub.set(i, i * i);
console.log("10 numeric subclass:", numsSub.get(3), numsSub.size);
const strsSub: Map<string, string> = new MyMap<string, string>();
strsSub.set("k", "v");
console.log("10 string-string subclass:", strsSub.get("k"), strsSub.size);
