// #7575: `m instanceof MyMap` was `false` for a `class MyMap extends Map {}`
// instance while `m instanceof Map` was `true` — the LEAF/intermediate class
// edge was lost and only the native base edge survived.
//
// The mechanism turned out to have nothing to do with Map/Set, or with native
// bases, or with prototype chains: it is MONOMORPHIZATION. Perry specializes
// generic classes, so `new MyMap<string, number>()` constructs a second class
// `MyMap$str_num` (`monomorph::mangle::generate_specialized_name`) carrying its
// own class id, and the instance is stamped with THAT id — while
// `x instanceof MyMap` resolves the RHS to the GENERIC's id, which appears
// nowhere in the specialization's parent chain. `class MyMap<K, V> extends
// Map<K, V>` is just the idiomatic spelling, which is why it surfaced there;
// section 9 below shows a generic class with a plain base, and one with NO
// base, failing identically before the fix.
//
// `test_gap_6325_map_set_subclass.ts` and `test_gap_7570_map_set_declared_base_type.ts`
// both asserted only `instanceof Map` / `instanceof Set`, which is why this
// survived them.

class MyMap<K, V> extends Map<K, V> {}
class MySet<T> extends Set<T> {}
class Plain {}
class SubPlain extends Plain {}
class Unrelated {}

// ── 1. the issue's exact repro ──
const a = new MyMap<string, number>();
console.log("1 unannotated:", a instanceof MyMap, a instanceof Map);

const b: Map<string, number> = new MyMap<string, number>();
console.log("1 annotated  :", b instanceof MyMap, b instanceof Map);

const sa = new MySet<number>();
console.log("1 set unann  :", sa instanceof MySet, sa instanceof Set);
const sb: Set<number> = new MySet<number>();
console.log("1 set ann    :", sb instanceof MySet, sb instanceof Set);

// ── 2. control: an ordinary hierarchy was never affected ──
const c: Plain = new SubPlain();
console.log("2 plain:", c instanceof SubPlain, c instanceof Plain);

// ── 3. negatives must stay negative ──
console.log("3 unrelated:", a instanceof Unrelated, a instanceof Set, sa instanceof Map);
console.log("3 base only:", new Map() instanceof MyMap, new Set() instanceof MySet);
console.log("3 real base:", new Map() instanceof Map, new Set() instanceof Set);

// ── 4. multi-level: the chain must be walked, not just its first edge ──
class A extends Map<string, number> {}
class B extends A {}
class C extends B {}
const bb = new B();
console.log("4 two level:", bb instanceof B, bb instanceof A, bb instanceof Map, bb instanceof C);
const cc = new C();
console.log("4 three lvl:", cc instanceof C, cc instanceof B, cc instanceof A, cc instanceof Map);

class SA extends Set<number> {}
class SB extends SA {}
const sbb = new SB();
console.log("4 set level:", sbb instanceof SB, sbb instanceof SA, sbb instanceof Set);

// ── 5. a subclass WITH its own constructor (explicit `super()` path) ──
class Tagged extends Map<string, number> {
  tag: string;
  constructor(tag: string) {
    super();
    this.tag = tag;
  }
}
const t = new Tagged("t");
console.log("5 explicit ctor:", t instanceof Tagged, t instanceof Map, t.tag);

class TaggedSub extends Tagged {}
const ts = new TaggedSub("u");
console.log("5 sub of ctor  :", ts instanceof TaggedSub, ts instanceof Tagged, ts instanceof Map);

// ── 6. seeded from an iterable — the surface must survive the branding fix ──
const seeded = new MyMap<string, number>([
  ["x", 1],
  ["y", 2],
]);
console.log("6 seeded:", seeded instanceof MyMap, seeded instanceof Map, seeded.size, seeded.get("x"));
seeded.set("z", 3);
console.log("6 mutate:", seeded.size, seeded.has("z"), seeded.get("z"), [...seeded.keys()].join(","));

const seededSet = new MySet<number>([1, 2, 2, 3]);
console.log("6 set    :", seededSet instanceof MySet, seededSet instanceof Set, seededSet.size, seededSet.has(2));

// ── 7. `Symbol.hasInstance` still takes precedence over the chain walk ──
class Never extends Map<string, number> {
  static [Symbol.hasInstance](_v: unknown): boolean {
    return false;
  }
}
const n = new Never();
console.log("7 hasInstance:", n instanceof Never, n instanceof Map);

class Always extends Map<string, number> {
  static [Symbol.hasInstance](_v: unknown): boolean {
    return true;
  }
}
console.log("7 always:", 42 instanceof Always, new Map() instanceof Always);

// ── 8. the dynamic RHS (`x instanceof ctorVar`) resolves the same way ──
const ctor: unknown = MyMap;
// deno-lint-ignore no-explicit-any
console.log("8 dynamic:", a instanceof (ctor as any), b instanceof (ctor as any));

// ── 9. it was never about Map/Set: a generic subclass of ANY base, and of no
//       base at all, failed identically. This is the real shape of the bug. ──
class PlainBase {}
class GenPlain<T> extends PlainBase {}
class GenNoBase<T> {}
class GenArr<T> extends Array<T> {}

const gp = new GenPlain<number>();
console.log("9 gen plain :", gp instanceof GenPlain, gp instanceof PlainBase);
const gn = new GenNoBase<number>();
console.log("9 gen nobase:", gn instanceof GenNoBase, gn instanceof PlainBase);
const ga = new GenArr<number>();
console.log("9 gen array :", ga instanceof GenArr, ga instanceof Array);
// ...and the NON-generic spellings, which already worked and must keep working.
class ConcPlain extends PlainBase {}
class ConcArr extends Array {}
console.log("9 concrete  :", new ConcPlain() instanceof ConcPlain, new ConcArr() instanceof ConcArr);
// The same generic class instantiated without type arguments takes a different
// specialization (none at all), and must answer the same way.
console.log("9 no-typearg:", new GenPlain() instanceof GenPlain, new GenArr() instanceof GenArr);
// Two specializations of one generic are siblings, not ancestors: `instanceof`
// must not have been widened into "shares a generic origin".
const gs = new GenPlain<string>();
console.log("9 siblings  :", gs instanceof GenPlain, gp instanceof GenPlain, gs instanceof PlainBase);

// ── 10. `instanceof` inside a function, over a parameter (no local type proof) ──
function isMine(v: unknown): string {
  return `${v instanceof MyMap} ${v instanceof Map} ${v instanceof MySet}`;
}
console.log("10 param:", isMine(a));
console.log("10 param:", isMine(sa));
console.log("10 param:", isMine(new Map()));
