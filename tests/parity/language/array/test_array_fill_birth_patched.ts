// The fill birth, negative controls around the builtin itself: an own
// `fill` put on one Array and a subclass with its own `fill`: neither is a
// fused fill `new Array(k).fill(v)`, and each must see its own function
// called.

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

// `Array.prototype.fill` itself cannot be replaced: the intrinsic prototype
// lock refuses a Replace of an image key (a TypeError here, where node
// installs the function), which is what makes the fused fill a fact. That
// difference from node predates the fill birth and is not asserted here.
const after = new Array<number>(4).fill(2);
console.log("after", after.join(","));
