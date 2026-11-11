// Separate program: Closed World must see a prototype iterator write.
// DEC 250 locks Array.prototype: the array adopts a user array prototype UAP
// (below Array.prototype), and the iterator write lands there.
const UAP: any = Object.create(Array.prototype);
const a: any = Object.setPrototypeOf([1, 2, 3], UAP);
a.pos = 1;
a.end = 2;
const original = Array.prototype[Symbol.iterator];
UAP[Symbol.iterator] = function () {
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
UAP[Symbol.iterator] = original;
console.log("prototype", JSON.stringify(seen), JSON.stringify(spread),
  JSON.stringify(from), JSON.stringify([first, second, rest]));
console.log("properties", a.pos, a.end, Object.keys(a).join("|"), JSON.stringify(a));
