// `for (const x of e)` over a non-literal source walks an ordinary array by
// index and keeps the iterator protocol for everything else. Each case checks
// a behavior the walk must share with the built-in array iterator.
const out: string[] = [];

// Live length: pushes during the loop are visited, truncation stops it.
const grow = [1, 2, 3];
for (const x of grow) {
  if (x < 6) grow.push(x + 3);
  out.push("g" + x);
}
const shrink = [1, 2, 3, 4, 5];
for (const x of shrink) {
  if (x === 2) shrink.length = 3;
  out.push("s" + x);
}

// Holes read undefined; early exits; continue.
const holey: (number | undefined)[] = [1, , 3];
for (const x of holey) out.push("h" + String(x));
function firstOver(xs: number[], limit: number): number {
  for (const x of xs) {
    if (x > limit) return x;
  }
  return -1;
}
out.push("r" + firstOver([1, 5, 9], 4) + "," + firstOver([1, 2], 4));
const skip = [1, 2, 3, 4];
for (const x of skip) {
  if (x % 2 === 0) continue;
  out.push("c" + x);
}

// Non-arrays keep the protocol.
for (const ch of "ab") out.push("t" + ch);
for (const [k, v] of new Map([["m", 1]])) out.push("m" + k + v);
for (const v of new Set([7, 8])) out.push("e" + v);
function* gen() {
  yield 1;
  yield 2;
}
for (const v of gen()) out.push("y" + v);

// An array with its own iterator, and a subclass that overrides one.
const own = [1, 2, 3];
(own as any)[Symbol.iterator] = function* () {
  yield 42;
};
for (const v of own) out.push("o" + v);
class Rev<T> extends Array<T> {
  *[Symbol.iterator]() {
    for (let i = this.length - 1; i >= 0; i--) yield this[i];
  }
}
const rev = new Rev<number>();
rev.push(1, 2, 3);
for (const v of rev) out.push("v" + v);

// Closures capture each iteration's binding.
const fns: (() => number)[] = [];
for (const x of [10, 20].map((n) => n + 1)) fns.push(() => x);
out.push("f" + fns.map((f) => f()).join("/"));

// A member source is evaluated once.
let reads = 0;
const holder = {
  get list() {
    reads++;
    return [1, 2];
  },
};
for (const x of holder.list) out.push("l" + x);
out.push("reads" + reads);

console.log(out.join(" "));
