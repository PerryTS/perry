// #6232: `class X extends Array` — instances are array-backed enough that the
// inherited Array instance methods, `Array.isArray`, indexed access, and
// iteration (for-of / spread / Array.from / concat) all work. The instance is a
// plain object routed to the runtime's spec-generic array-like engine.
//
// Validated byte-for-byte against `node --experimental-strip-types`.

class Stack<T> extends Array<T> {
  peek(): T | undefined {
    return this[this.length - 1];
  }
}

const s = new Stack<number>();
console.log("isArray:", Array.isArray(s));
console.log("instanceof Array:", s instanceof Array);

s.push(10);
s.push(20);
s.push(30);
console.log("push length:", s.length);
console.log("index:", s[0], s[2]);
console.log("peek (own method):", s.peek());

console.log("pop:", s.pop(), "length:", s.length);
s.push(40);
s.push(50);

console.log("join:", s.join(","));
console.log("at(-1):", s.at(-1));
console.log("indexOf(40):", s.indexOf(40));
console.log("includes(50):", s.includes(50));
console.log("slice(1):", s.slice(1).join(","));
console.log("map:", s.map((x: number) => x * 2).join(","));
console.log("filter:", s.filter((x: number) => x >= 40).join(","));
console.log("reduce:", s.reduce((a: number, b: number) => a + b, 0));

let sum = 0;
s.forEach((x: number) => {
  sum += x;
});
console.log("forEach sum:", sum);

// iteration
const viaForOf: number[] = [];
for (const x of s) viaForOf.push(x);
console.log("for-of:", viaForOf.join(","));
console.log("spread:", [...s].join(","));
console.log("Array.from:", Array.from(s).join(","));
console.log("concat:", [1, 2].concat(s as any).join(","));

// isArray on non-arrays stays correct
console.log("isArray non:", Array.isArray("x"), Array.isArray({}), Array.isArray(5));

// non-generic subclass instanceof
class Nums extends Array {}
const n = new Nums();
console.log("subclass instanceof:", n instanceof Nums, n instanceof Array);

// a user `[Symbol.iterator]` override wins over the default element iteration
class Custom extends Array {
  *[Symbol.iterator](): IterableIterator<number> {
    yield 100;
    yield 200;
  }
}
const cu = new Custom();
cu.push(1);
cu.push(2);
console.log("override spread:", [...cu].join(","));
console.log("override from:", Array.from(cu).join(","));
