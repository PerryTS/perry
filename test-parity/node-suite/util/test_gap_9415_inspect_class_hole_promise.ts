// #9415 — three `console.log` / `util.inspect` value-representation defects,
// byte-compared against `node --experimental-strip-types`.
//
//   1. a class value printed as a bare integer (its class id): a class ref is
//      an INT32-tagged NaN box, and every display ladder's `is_int32()` arm
//      printed `as_int32()`. `console.log(class Klass {})` said `1`.
//   2. an array hole printed as `NaN`: `TAG_HOLE`'s bit pattern IS a NaN, so a
//      ladder that falls through to "must be a regular number" turns holes into
//      NaN. `console.log(new Array(3))` said `[ NaN, NaN, NaN ]`.
//   3. every promise reported `<pending>`: the state byte was never read.
//
// Also covers the same hole sentinel in the Map/Set inspect walks, where a
// tombstoned `delete` left `TAG_HOLE` in the raw slot AND the `0..size` bound
// stopped short of the live tail (`new Set([1,2,3])` after `delete(1)` printed
// `Set(2) { NaN, 2 }`).
//
// Deliberately NOT covered, because each is a separate pre-existing divergence
// this fix does not touch (each is called out in the report):
//   - `String(SomeClass)` / `util.format("%s", SomeClass)`: perry answers
//     `function Klass() { [native code] }`, node the class source text.
//   - `util.format("%s", someObject)`: node inspects an object with no user
//     `toString`, perry uses `String(value)`.
//   - `util.format("%o", …)`: node's `%o` is `showHidden: true`, which perry
//     does not implement for arrays or classes.

import util from "node:util";

class Klass {}
class Base {}
class Sub extends Base {}
class Holder {
  a: number;
  constructor() {
    this.a = 1;
  }
}

// --- 1. class values ---
console.log("class-named", Klass);
console.log("class-extends", Sub);
console.log("class-anonymous", class {});
console.log("class-anonymous-extends", class extends Base {});
console.log("inspect-named", util.inspect(Klass));
console.log("inspect-extends", util.inspect(Sub));
console.log("format-O", util.format("%O", Klass));
console.log("class-in-array", [Klass, Sub]);
console.log("class-in-object", { k: Klass });
console.error("class-error", Klass);
console.warn("class-warn", Klass);

// An INSTANCE is an ordinary object and must not be labelled `[class …]`.
console.log("instance", new Holder());
console.log("instance-inspect", util.inspect(new Holder()));
console.log("instance-not-class", util.inspect(new Holder()).startsWith("[class"));

// A plain integer must stay an integer. Class ids are small and sequential —
// `Klass` above is id 1, `Base` id 2 — and a ClassRef is bit-identical to the
// int32 with the same payload, so these are the lines an over-eager fix (one
// that labels every INT32-tagged value without the class-registry probe) gets
// wrong. They print integers because a JS number is a plain f64 double and
// never reaches the INT32 display arm at all; the registry probe is the second
// line of defence, not the only one.
console.log("plain-int", 49);
console.log("plain-ints", 1, 2, 3, 4, 5);
console.log("collides-with-class-id", 1);
console.log("collides-with-class-id-2", 2);
const asAny: any = 1;
console.log("collides-via-any", asAny);
console.log("length-is-a-number", [9].length);
console.log("indexof-is-a-number", ["a", "b"].indexOf("b"));
console.log("bitwise-is-a-number", 3 | 0);
console.log("charcode-is-a-number", "A".charCodeAt(0));
console.log("plain-int-array", [10, 20, 30]);
console.log("int-expression", 6 * 8 + 1);

// Two DISTINCT classes that share a name render identically, so deep-equality
// must not be decided by the rendering.
class Twin {}
function makeTwin() {
  class Twin {}
  return Twin;
}
console.log("same-name-classes-differ", util.isDeepStrictEqual(Twin, makeTwin()));
console.log("same-class-equals-itself", util.isDeepStrictEqual(Twin, Twin));
console.log("equal-numbers-still-equal", util.isDeepStrictEqual(1, 1));
console.log("unequal-numbers-differ", util.isDeepStrictEqual(1, 2));

// --- 2. array holes ---
console.log("new-array", new Array(3));
console.log("elision", [1, , 3]);
const deleted = [1, 2, 3];
delete deleted[0];
console.log("delete", deleted);
const trailing: number[] = [1];
trailing[3] = 4;
console.log("grown", trailing);
console.log("nested-in-object", { h: new Array(3) });
console.log("nested-in-array", [new Array(2)]);
console.log("inspect-holes", util.inspect(new Array(3)));
console.log("format-O-holes", util.format("%O", new Array(3)));
console.log("empty", new Array(0));
console.log("one-hole", new Array(1));
console.log("mixed-runs", [1, , 2, , , 3]);
console.error("holes-error", new Array(3));
console.warn("holes-warn", new Array(3));
// Seven SLOTS but ONE entry: the single-line/multi-line decision counts the
// entries Node prints, not the array's length.
console.log("seven-holes", new Array(7));
console.log("map-over-holes", new Array(3).map((x) => x));
// A hole is still not a value anywhere else.
console.log("holes-json", JSON.stringify(new Array(3)));
console.log("holes-string", String([1, , 3]));
console.log("holes-length", new Array(3).length);
console.log("holes-keys", Object.keys(new Array(3)).length);

// --- 3. promise state ---
const settled = Promise.resolve(1);
console.log("resolved", settled);
console.log("pending", new Promise(() => {}));
console.log("resolved-inspect", util.inspect(Promise.resolve("x")));
console.log("resolved-undefined", Promise.resolve(undefined));
console.log("resolved-object", Promise.resolve({ a: 1 }));
console.log("promise-in-array", [Promise.resolve(1)]);
console.log("promise-in-object", { p: Promise.resolve(1) });
const rejected = Promise.reject(42);
rejected.catch(() => {});
console.log("rejected", rejected);

// --- Map / Set tombstones ---
const numbers = new Set([1, 2, 3]);
numbers.delete(1);
console.log("set-delete", numbers);
const strings = new Set(["a", "b", "c"]);
strings.delete("b");
console.log("set-delete-middle", strings);
const pairs = new Map<string, number>([
  ["a", 1],
  ["b", 2],
]);
pairs.delete("a");
console.log("map-delete", pairs);
console.log("set-intact", new Set([1, 2]));
console.log("map-intact", new Map([["k", 1]]));
console.log("set-spread", [...numbers], numbers.size);
