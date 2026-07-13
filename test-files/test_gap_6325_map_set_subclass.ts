// #6325: `class M extends Map {}` / `extends Set {}` — an IMPLICIT (no own
// constructor) subclass of Map/Set produced an instance with no collection
// storage and therefore no collection methods at all: `m.set("a", 1)` threw
// `TypeError: set is not a function`.
//
// Perry models a Map/Set subclass instance as a plain object carrying a hidden
// BACKING `MapHeader`/`SetHeader` (see `object/map_set_subclass.rs`); the
// backing is allocated by `js_map_set_subclass_init`, which only ever ran from
// the EXPLICIT-`super()` lowering. A class with no own constructor never calls
// `super()` in source, so the implicit-default-ctor `new` path skipped the init
// and the instance stayed storage-less.
//
// The init now fires wherever the class CHAIN reaches Map/Set, not only where a
// literal `super()` appears.

// ── the issue's exact repro: no ctor, no override ──
class M extends Map {}
const m = new M();
m.set("a", 1);
console.log("map subclass get:", m.get("a"), "size:", m.size);

class S extends Set {}
const s = new S();
s.add("x");
console.log("set subclass has:", s.has("x"), "size:", s.size);

// ── the registry parent edge is wired, so `instanceof` holds ──
console.log("instanceof:", m instanceof Map, s instanceof Set);

// ── explicit-ctor control (this path already worked — must keep working) ──
class WithCtor extends Map {
  tag: string;
  constructor(tag: string) {
    super();
    this.tag = tag;
  }
}
const wc = new WithCtor("t");
wc.set("b", 2);
console.log("explicit ctor:", wc.get("b"), wc.size, wc.tag);

// ── constructor argument seeds the backing on the implicit path too ──
class Seeded extends Map<string, number> {}
const seeded = new Seeded([
  ["k", 9],
  ["j", 8],
]);
console.log("seeded:", seeded.get("k"), seeded.get("j"), seeded.size);

class SeededSet extends Set<number> {}
const ss = new SeededSet([1, 2, 2, 3]);
console.log("seeded set:", ss.size, ss.has(2), ss.has(4));

// ── INDIRECT subclass: the chain reaches Map one hop away ──
class MidMap extends Map {}
class LeafMap extends MidMap {}
const leaf = new LeafMap();
leaf.set("z", 3);
console.log("indirect:", leaf.get("z"), leaf.size, leaf instanceof Map);

// ── the full collection surface, iteration included ──
const all = new M();
all.set("one", 1);
all.set("two", 2);
const keys: string[] = [];
all.forEach((_v: number, k: string) => keys.push(k));
console.log("forEach keys:", keys.join(","));
console.log("spread:", JSON.stringify([...all]));
console.log("has/delete:", all.has("one"), all.delete("one"), all.size);
all.clear();
console.log("cleared:", all.size);

// ── a subclass OVERRIDE must win over the backing surface, and `super.<m>()`
//    from inside it must reach the base (the #6316/#6322 ordering discipline) ──
class Counting extends Map<string, number> {
  writes = 0;
  set(k: string, v: number): this {
    this.writes++;
    return super.set(k, v);
  }
  get(k: string): number | undefined {
    const raw = super.get(k);
    return raw === undefined ? undefined : raw * 10;
  }
}
const counting = new Counting();
counting.set("a", 1);
counting.set("b", 2);
console.log("override:", counting.get("a"), counting.get("b"), counting.get("zz"));
console.log("override writes:", counting.writes, "size:", counting.size);

class LoudSet extends Set<number> {
  added: number[] = [];
  add(v: number): this {
    this.added.push(v);
    return super.add(v);
  }
}
const loud = new LoudSet();
loud.add(4);
loud.add(5);
console.log("set override:", loud.added.join(","), loud.size, loud.has(5));

// ── a non-collection user method on a Map subclass still dispatches normally ──
class Extra extends Map<string, number> {
  total(): number {
    let t = 0;
    this.forEach((v: number) => {
      t += v;
    });
    return t;
  }
}
const extra = new Extra();
extra.set("p", 3);
extra.set("q", 4);
console.log("user method:", extra.total(), extra.size);
