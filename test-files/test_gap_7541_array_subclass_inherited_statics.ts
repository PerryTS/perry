// #7541 — `[...MyArr.from([1,2,3])]` threw `TypeError: value is not iterable`.
//
// The spread was never the problem: `Array.from` / `Array.of` / `Array.isArray`
// are folded in the HIR on the LITERAL identifier `Array`, so a SUBCLASS
// receiver matched nothing, and `js_class_static_method_call`'s miss-fallback
// returns the RECEIVER — making `MyArr.from([1,2,3])` evaluate to the class ref,
// which is genuinely not iterable. Directly-constructed instances always
// spread fine; only the inherited-static-produced ones failed.
//
// KNOWN GAP, deliberately not asserted here: the property-GET form
// (`typeof MyArr.from`) still reports `undefined` — only the CALL form is
// dispatched. Same for `sub instanceof MyArr`, a pre-existing class-registry
// parent-edge gap (#7575's Map/Set sibling).

class MyArr extends Array {}
class Indirect extends MyArr {}

// The issue's exact repro.
const sub = MyArr.from([1, 2, 3]);
console.log([...sub]);
console.log(Array.isArray([...sub]));

// The statics themselves.
console.log("from     ", Array.isArray(sub), sub.length, sub.join(","));
const mapped = MyArr.from([1, 2, 3], (v: number) => v * 10);
console.log("from+map ", mapped.length, mapped.join(","));
const fromSet = MyArr.from(new Set([4, 5, 6]));
console.log("from set ", fromSet.length, fromSet.join(","));
const fromLike = MyArr.from({ length: 2, 0: "a", 1: "b" } as ArrayLike<string>);
console.log("from like", fromLike.length, fromLike.join(","));
const ofd = MyArr.of(7, 8, 9);
console.log("of       ", ofd.length, ofd.join(","));
console.log("isArray  ", MyArr.isArray([]), MyArr.isArray(1));

// An INDIRECT subclass resolves through the same chain walk.
const ind = Indirect.from([1, 2]);
console.log("indirect ", Array.isArray(ind), ind.length, ind.join(","));

// Every iteration surface on a static-produced instance.
const it = MyArr.from([10, 20, 30]);
const acc: number[] = [];
for (const v of it) {
  acc.push(v);
}
console.log("for-of   ", acc.join(","));
console.log("spread   ", [...it].join(","));
console.log("Array.from", Array.from(it).join(","));
const [a0, a1] = it;
console.log("destr    ", a0, a1);
console.log("map      ", it.map((v) => v + 1).join(","));
console.log("index    ", it[0], it[2], it.length);

// Controls: the base intrinsic and a non-Array class are untouched.
console.log("base from", Array.from([1, 2]).join(","));
console.log("base of  ", Array.of(3, 4).join(","));
class Other {
  static make(): string {
    return "other";
  }
}
class OtherSub extends Other {}
console.log("user stat", OtherSub.make());
console.log("done");
