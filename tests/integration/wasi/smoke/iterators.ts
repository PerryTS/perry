// Generators, iterator protocol, iterator helpers.
function* gen() {
  yield 1;
  yield 2;
  yield 3;
}
for (const v of gen()) console.log(v);
console.log([...gen()].map((v) => v * 10));
class Range {
  lo: number;
  hi: number;
  constructor(lo: number, hi: number) {
    this.lo = lo;
    this.hi = hi;
  }
  *[Symbol.iterator]() {
    for (let i = this.lo; i < this.hi; i++) yield i;
  }
}
console.log([...new Range(2, 5)]);
console.log([...gen().map((v) => v + 1)]);
