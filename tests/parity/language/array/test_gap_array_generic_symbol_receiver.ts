// Generic Array methods apply ToObject to a Symbol primitive.
const s = Symbol("s");
console.log("slice", Array.prototype.slice.call(s).length);
console.log("reflect-slice", Reflect.apply(Array.prototype.slice, s, []).length);
console.log("map", Array.prototype.map.call(s, () => 99).length);
console.log("filter", Array.prototype.filter.call(s, () => true).length);
console.log("search", Array.prototype.indexOf.call(s, 1), Array.prototype.lastIndexOf.call(s, 1), Array.prototype.includes.call(s, 1));
console.log("predicates", Array.prototype.every.call(s, () => false), Array.prototype.some.call(s, () => true));
console.log("find", Array.prototype.find.call(s, () => true), Array.prototype.findIndex.call(s, () => true));
console.log("findLast", Array.prototype.findLast.call(s, () => true), Array.prototype.findLastIndex.call(s, () => true));
console.log("at-join", Array.prototype.at.call(s, 0), Array.prototype.join.call(s, ","));
console.log("reduce", Array.prototype.reduce.call(s, (a: number) => a + 1, 42), Array.prototype.reduceRight.call(s, (a: number) => a + 1, 43));
let visited = 0;
console.log("forEach", Array.prototype.forEach.call(s, () => { visited++; }), visited);
console.log("flat", Array.prototype.flat.call(s).length, Array.prototype.flatMap.call(s, () => [1]).length);
console.log("copies", Array.prototype.toReversed.call(s).length, Array.prototype.toSorted.call(s).length);
console.log("iterators", Array.prototype.values.call(s).next().done, Array.prototype.keys.call(s).next().done, Array.prototype.entries.call(s).next().done);
console.log("pop-shift", Array.prototype.pop.call(s), Array.prototype.shift.call(s));
console.log("push-unshift", Array.prototype.push.call(s, 1, 2), Array.prototype.unshift.call(s, 3));
console.log("splice", Array.prototype.splice.call(s, 0, 0, 1).length);
console.log("wrappers", Array.prototype.reverse.call(s) instanceof Symbol, Array.prototype.sort.call(s) instanceof Symbol, Array.prototype.fill.call(s, 1) instanceof Symbol, Array.prototype.copyWithin.call(s, 0, 1) instanceof Symbol);
const concat = Array.prototype.concat.call(s, 1);
console.log("concat", concat.length, typeof concat[0], concat[0] instanceof Symbol, concat[1]);
const held = Array.prototype.reverse;
console.log("held", held.call(s) instanceof Symbol);
console.log("apply", Reflect.apply(Array.prototype.sort, s, []) instanceof Symbol);
try {
  Array.prototype.with.call(s, 0, 1);
} catch (e) {
  console.log("with", e instanceof RangeError);
}
// The wrapper's prototype supplies array-like properties through ordinary Get.
Object.defineProperty(Symbol.prototype, "length", { value: 1, configurable: true });
Object.defineProperty(Symbol.prototype, "0", { value: "inherited", configurable: true });
console.log("inherited-slice", Reflect.apply(Array.prototype.slice, s, []).join(","));
console.log("inherited-iterator", Reflect.apply(Array.prototype.values, s, []).next().value);
delete (Symbol.prototype as any).length;
delete (Symbol.prototype as any)[0];
