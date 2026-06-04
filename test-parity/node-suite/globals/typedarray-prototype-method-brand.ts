// %TypedArray%.prototype methods are not generic Array helpers: extracted
// calls must validate that `this` is a TypedArray receiver.

function show(label: string, fn: () => unknown): void {
  try {
    console.log(label + ":", String(fn()));
  } catch (err) {
    console.log(label + ":", (err as Error).name);
  }
}

const map = Uint16Array.prototype.map;
const mapped: any = map.call(new Uint16Array([1, 2]), (value: number) => value * 2);
console.log("map typed:", mapped instanceof Uint16Array, mapped.length);
show("map array receiver", () => map.call([1, 2] as any, (value: number) => value * 2));

const filter = Int8Array.prototype.filter;
const filtered: any = filter.call(new Int8Array([-1, 2, 3]), (value: number) => value > 0);
console.log("filter typed:", filtered instanceof Int8Array, filtered.length);
show("filter array receiver", () => filter.call([1, 2] as any, (value: number) => value > 1));

const reduce = Float32Array.prototype.reduce;
console.log(
  "reduce typed:",
  reduce.call(new Float32Array([1.5, 2.5]), (acc: number, value: number) => acc + value, 0),
);
show("reduce array receiver", () =>
  reduce.call([1, 2] as any, (acc: number, value: number) => acc + value, 0)
);

const generic = Array.prototype.map.call({ 0: 3, 1: 4, length: 2 }, (value: number) => value + 1);
console.log("array prototype generic:", generic.join("|"));
