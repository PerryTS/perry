// Separate program: Closed World must see a prototype iterator write.
const a: any = [1, 2, 3];
a.pos = 1;
a.end = 2;
const original = Array.prototype[Symbol.iterator];
Array.prototype[Symbol.iterator] = function () {
  let n = 0;
  const source: any = this;
  return { next() {
    return n < source.length ? { value: source[n++] * 10, done: false } : { done: true };
  } } as any;
};
const seen: number[] = [];
for (const value of a) seen.push(value);
const spread = [...a];
const from = Array.from(a);
const [first, second, ...rest] = a;
Array.prototype[Symbol.iterator] = original;
console.log("prototype", JSON.stringify(seen), JSON.stringify(spread),
  JSON.stringify(from), JSON.stringify([first, second, rest]));
console.log("properties", a.pos, a.end, Object.keys(a).join("|"), JSON.stringify(a));
