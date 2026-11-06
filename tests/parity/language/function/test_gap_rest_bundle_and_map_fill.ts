// Rest/`arguments` bundles are built the way an array literal is, and
// `Array.prototype.map`'s plain-array fill resolves the result header once.
// Both keep the element-kind, hole and barrier protocol of the paths they
// replace, so this pins what a caller can observe: identity, length, element
// values and kinds, holes, and what happens when a callback mutates or grows
// the array it is filling.

function rest(...xs: any[]): any[] {
  return xs;
}
function restAfterFixed(a: number, b: number, ...xs: any[]): string {
  return `${a}|${b}|${xs.length}|${xs.join(",")}`;
}
function argsObject(): any {
  // eslint-disable-next-line prefer-rest-params
  return arguments;
}

// Numbers, the shape the raw-f64 layout claims.
console.log("nums", JSON.stringify(rest(1, 2, 3)), rest(1, 2, 3).length);
console.log("empty", JSON.stringify(rest()), rest().length, Array.isArray(rest()));
console.log("fixed+rest", restAfterFixed(1, 2, 3, 4, 5));
console.log("no-rest-args", restAfterFixed(1, 2));

// Mixed kinds must retire the numeric claim, not store raw bits.
const obj = { tag: "o" };
const mixed = rest(1, "two", null, undefined, true, obj, 6.5, -0, NaN, Infinity);
console.log("mixed", mixed.length, typeof mixed[1], mixed[2], mixed[3], mixed[4]);
console.log("mixed-obj-identity", mixed[5] === obj, Object.is(mixed[7], -0), mixed[8] !== mixed[8]);
console.log("mixed-json", JSON.stringify(mixed));

// A rest array is an ordinary, extensible, mutable array.
const r = rest(1, 2, 3);
r.push(4);
r[6] = 7;
console.log("mutable", JSON.stringify(r), r.length, 5 in r, JSON.stringify(Object.keys(r)));

// Past the inline width (16), the older construction still applies.
const wide = rest(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18);
console.log("wide", wide.length, wide[0], wide[17], JSON.stringify(wide.slice(15)));

// `arguments` keeps its own identity and spread behaviour.
const a = argsObject(1, 2, 3);
console.log("arguments", a.length, a[0], a[2], JSON.stringify(Array.from(a)));

// Rest of objects: every element must survive a collection that runs while the
// bundle is still being built by the next call's arguments.
function makeTag(i: number): any {
  return { i, pad: new Array(8).fill(i) };
}
const objs = rest(makeTag(0), makeTag(1), makeTag(2), makeTag(3));
console.log("obj-rest", objs.length, objs.map((o: any) => o.i).join(","));

// ---- map ----------------------------------------------------------------

const nums = [1, 2, 3, 4];
console.log("map-num", JSON.stringify(nums.map((v) => v * 2)));
console.log("map-to-string", JSON.stringify(nums.map((v) => `n${v}`)));
console.log("map-to-obj", JSON.stringify(nums.map((v) => ({ v }))));
console.log("map-mixed", JSON.stringify(nums.map((v) => (v % 2 === 0 ? v : String(v)))));
console.log("map-negzero", Object.is(nums.map(() => -0)[0], -0));
console.log("map-nan", nums.map(() => NaN).every((x) => x !== x));

// Holes stay holes; the callback is not called for them.
const holey = [1, , 3];
const mappedHoley = holey.map((v) => (v as number) * 10);
console.log("map-holes", JSON.stringify(mappedHoley), 1 in mappedHoley, mappedHoley.length);

// A callback that mutates the source, grows it, or allocates heavily.
const src = [1, 2, 3];
const mutated = src.map((v, i) => {
  if (i === 0) {
    src.push(99);
    src[2] = 42;
  }
  return v;
});
console.log("map-mutating", JSON.stringify(mutated), JSON.stringify(src));

const allocating = [1, 2, 3, 4, 5, 6].map((v) => {
  const junk = new Array(64).fill({ v });
  return junk.length + v;
});
console.log("map-allocating", JSON.stringify(allocating));

// Longer than the map fill's 64-element resolved-header branch.
const long = new Array(80).fill(0).map((_, i) => i);
const longMapped = long.map((v) => v + 0.5);
console.log("map-long", longMapped.length, longMapped[0], longMapped[79]);
const longObjs = long.map((v) => ({ v }));
console.log("map-long-obj", longObjs.length, longObjs[79].v, typeof longObjs[0]);

// Species and subclass results take the unchanged [[Set]] path. Only the
// VALUES are asserted: `map` on a subclass receiver does not preserve the
// subclass in perry today (`subMapped instanceof MyArr` is false where node
// says true), a pre-existing gap this fixture must not start failing on.
class MyArr extends Array {}
const sub = MyArr.from([1, 2, 3]) as any;
const subMapped = sub.map((v: number) => v + 1);
console.log("map-species", JSON.stringify(Array.from(subMapped)));

// Frozen source, and a result read back through every element kind.
const frozen = Object.freeze([1, 2, 3]);
console.log("map-frozen-src", JSON.stringify(frozen.map((v) => v + 1)));
const kinds = [0, "s", null, undefined, true, { o: 1 }, [1]].map((v) => typeof v);
console.log("map-kinds", JSON.stringify(kinds));
