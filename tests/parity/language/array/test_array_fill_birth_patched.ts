// The fill birth, negative controls around the builtin itself: an own
// `fill` put on one Array, a subclass with its own `fill`, and at the end
// `Array.prototype.fill` replaced by the program. The fused fill
// `new Array(k).fill(v)` calls the native `fill` only while the builtin is
// in place; a program that replaces it must see its own function called.

const before = new Array<number>(4).fill(1);
console.log("before", before.join(","));

// An own `fill` on one named Array: not a fused fill.
const own = new Array<number>(4);
(own as unknown as { fill: (v: number) => string }).fill = (v: number) => "own " + v;
console.log("own fill", (own as unknown as { fill: (v: number) => string }).fill(3));

// A subclass with its own `fill`.
class Loud extends Array<number> {
  fill(v: number): this {
    console.log("loud fill", v, this.length);
    return super.fill(v + 1);
  }
}
const loud = new Loud(3).fill(5);
console.log("subclass", loud.join(","), loud instanceof Loud);

// The prototype's `fill` replaced: every later fused fill is the program's.
let calls = 0;
const native = Array.prototype.fill;
let replaced = true;
try {
  (Array.prototype as unknown as { fill: unknown }).fill = function (this: number[], v: number) {
    calls += 1;
    for (let i = 0; i < this.length; i++) this[i] = v * 10;
    return this;
  };
} catch (e) {
  replaced = false;
  console.log("replace threw", (e as Error).name);
}
const after = new Array<number>(4).fill(2);
console.log("after", after.join(","), calls, replaced);
const afterBig = new Array<number>(40).fill(3);
console.log("after big", afterBig[0], afterBig[39], calls);
(Array.prototype as unknown as { fill: unknown }).fill = native;
const restored = new Array<number>(4).fill(2);
console.log("restored", restored.join(","), calls);
