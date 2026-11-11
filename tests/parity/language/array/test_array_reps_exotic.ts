// Values that look like arrays but are not plain ones (DESIGN-p4l1 §5.4
// fixture 8): a Proxy joined into a receiver that also holds plain arrays,
// and an Array subclass.

function total(a: number[], n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) {
    const k = i;
    s += a[k] ?? 100;
  }
  return s;
}

const plain = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18];
let traps = 0;
const proxied = new Proxy([1, 2], {
  get(target, key, receiver) {
    traps++;
    return Reflect.get(target, key, receiver);
  },
});
function pick(c: number): number[] {
  return c === 0 ? plain : proxied;
}
console.log("plain", total(pick(0), 18), traps);
console.log("proxy", total(pick(1), 4), traps > 0, Array.isArray(pick(1)));

// A transparent Proxy with no traps.
const bare = new Proxy([5, 6, 7], {});
const both = process.argv.length > 100 ? plain : bare;
console.log("bare", total(both, 4), both.length);

class Stack extends Array<number> {
  top(): number {
    return this[this.length - 1] ?? -1;
  }
}
const stack = new Stack();
for (let i = 0; i < 20; i++) stack.push(i);
console.log("subclass", total(stack, 21), stack.top(), stack.length, stack instanceof Stack);
const mixed: number[] = process.argv.length > 100 ? plain : stack;
console.log("mixed", total(mixed, 20), mixed.length);

// An array-like object behind an array type.
const like = { 0: 4, 1: 5, length: 2 } as unknown as number[];
const maybe: number[] = process.argv.length > 100 ? plain : like;
console.log("like", total(maybe, 3), maybe.length);

// A typed array behind an array type.
const typed = new Float64Array([1.5, 2.5, 3.5]) as unknown as number[];
const maybeTyped: number[] = process.argv.length > 100 ? plain : typed;
console.log("typed", total(maybeTyped, 4), maybeTyped.length);
