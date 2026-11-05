// #7563: `arr[Symbol.iterator]` read an ARRAY's `capacity` field as a `class_id`.
//
// `ObjectHeader` is `{ object_type: u32, class_id: u32, … }` and `ArrayHeader`
// is `{ length: u32, capacity: u32 }`, so the two u32s at offset 4 alias: an
// N-capacity array read as an `ObjectHeader` reports "class id N".
//
// `arr[Symbol.iterator]` resolves through `js_class_method_bind(arr, "values")`
// (`symbol/get.rs`), and that bound-method builder read `class_id` off the
// receiver with a BARE `(*obj).class_id` instead of the guarded
// `js_object_get_class_id` accessor (which rejects any allocation whose
// `GcHeader.obj_type` is not `GC_TYPE_OBJECT`). So whenever the class whose id
// equalled the array's capacity happened to own a `values` method, the array's
// iterator resolved to THAT class's method.
//
// The issue was reported as a `class X extends Map` bug, but Map is incidental:
// the only thing that matters is a class owning a method named `values`. When
// that class is also the one whose `values` body builds the array literal, the
// method calls itself until the stack guard page — a SIGSEGV.

// ── the issue's exact reproducer ──
class MyMap<K, V> extends Map<K, V> {
  values(): IterableIterator<V> {
    return [777 as unknown as V][Symbol.iterator]();
  }
}

const m = new MyMap<string, number>();
m.set("q", 5);
console.log("size:", m.size);
console.log("get:", m.get("q"));
const out: number[] = [];
for (const v of m.values()) out.push(v);
console.log("values:", out.join(","));

// ── the same crash with NO `Map` anywhere: a plain class whose method is named
//    `values` and whose body iterates an array literal. Before the fix the
//    one-element literal read back as class id 1 — the class itself — so
//    `values` called `values` until the stack overflowed. ──
class Plain {
  values(): IterableIterator<number> {
    return [777][Symbol.iterator]();
  }
}
console.log("plain:", [...new Plain().values()].join(","));

// ── the non-recursive form of the same mis-dispatch: the array's capacity
//    selects a DIFFERENT class that owns `values`. Pre-fix the 2-element
//    literal resolved to `B.values` (a number), and the spread threw
//    "value is not iterable". ──
class A {
  one(): IterableIterator<number> {
    return [1][Symbol.iterator]();
  }
  two(): IterableIterator<number> {
    return [1, 2][Symbol.iterator]();
  }
  three(): IterableIterator<number> {
    return [1, 2, 3][Symbol.iterator]();
  }
}
class B {
  values(): number {
    return 42;
  }
}
const a = new A();
console.log("cap1:", [...a.one()].join(","));
console.log("cap2:", [...a.two()].join(","));
console.log("cap3:", [...a.three()].join(","));
console.log("B.values:", new B().values());

// ── a free function (no class in scope) always worked; keep it covered so a
//    future narrowing of the guard cannot silently break the ordinary path. ──
function free(): IterableIterator<number> {
  return [888][Symbol.iterator]();
}
console.log("free:", [...free()].join(","));

// ── the array `values`/`keys`/`entries` surface itself must stay intact ──
const plainArr = [10, 20, 30];
console.log("arr values:", [...plainArr.values()].join(","));
console.log("arr keys:", [...plainArr.keys()].join(","));
console.log("arr entries:", JSON.stringify([...plainArr.entries()]));
console.log("arr @@iterator:", [...plainArr[Symbol.iterator]()].join(","));

// ── native-base subclass overrides, the family the issue reported against ──
class MapKeysOverride extends Map<string, number> {
  keys(): IterableIterator<string> {
    return ["kk"][Symbol.iterator]();
  }
}
class MapEntriesOverride extends Map<string, number> {
  entries(): IterableIterator<[string, number]> {
    return ([["ee", 1]] as [string, number][])[Symbol.iterator]();
  }
}
class MapIterOverride extends Map<string, number> {
  *[Symbol.iterator](): IterableIterator<[string, number]> {
    yield ["ii", 9];
  }
}
const mk = new MapKeysOverride();
mk.set("q", 5);
console.log("map keys override:", [...mk.keys()].join(","));
const me = new MapEntriesOverride();
me.set("q", 5);
console.log("map entries override:", JSON.stringify([...me.entries()]));
const mi = new MapIterOverride();
mi.set("q", 5);
console.log("map @@iterator override:", JSON.stringify([...mi]));

class SetValuesOverride extends Set<number> {
  values(): IterableIterator<number> {
    return [111][Symbol.iterator]();
  }
}
class SetIterOverride extends Set<number> {
  *[Symbol.iterator](): IterableIterator<number> {
    yield 444;
  }
}
const sv = new SetValuesOverride();
sv.add(1);
console.log("set values override:", [...sv.values()].join(","));
const si = new SetIterOverride();
si.add(1);
console.log("set @@iterator override:", [...si].join(","));

class ArrValuesOverride extends Array<number> {
  values(): IterableIterator<number> {
    return [555][Symbol.iterator]();
  }
}
const av = new ArrValuesOverride();
av.push(1);
console.log("array values override:", [...av.values()].join(","));

// ── INDIRECT subclass and a class EXPRESSION: the two shapes CLAUDE.md's
//    "native base-class subclassing" note calls out as historically lossy. ──
class MidMap extends Map<string, number> {}
class LeafMap extends MidMap {
  values(): IterableIterator<number> {
    return [888][Symbol.iterator]();
  }
}
const lm = new LeafMap();
lm.set("z", 3);
console.log("indirect override:", [...lm.values()].join(","));

const ExprMap = class extends Map<string, number> {
  values(): IterableIterator<number> {
    return [999][Symbol.iterator]();
  }
};
const em = new ExprMap();
em.set("z", 3);
console.log("class-expression override:", [...em.values()].join(","));

// ── non-overriding subclasses keep the built-in surface ──
class PlainMap extends Map<string, number> {}
const pm = new PlainMap();
pm.set("a", 1);
pm.set("b", 2);
console.log("no-override values:", [...pm.values()].join(","));
console.log("no-override keys:", [...pm.keys()].join(","));
console.log("no-override entries:", JSON.stringify([...pm.entries()]));
console.log("no-override spread:", JSON.stringify([...pm]));

// ── `super.<m>()` from inside an override still reaches the native base ──
class SuperMap extends Map<string, number> {
  values(): IterableIterator<number> {
    return super.values();
  }
}
const sm = new SuperMap();
sm.set("a", 1);
sm.set("b", 2);
console.log("super.values():", [...sm.values()].join(","));

console.log("done");
