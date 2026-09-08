// Exercise declaration/expression vtables, own overrides, prototype mutation,
// IteratorClose, and symbol enumeration in one deterministic matrix.

class DeclaredRange {
  lo: number;
  hi: number;
  constructor(lo: number, hi: number) {
    this.lo = lo;
    this.hi = hi;
  }
  *[Symbol.iterator]() {
    for (let i = this.lo; i <= this.hi; i++) yield i;
  }
}

const ExpressionRange = class {
  *[Symbol.iterator]() {
    yield "expr-a";
    yield "expr-b";
  }
};

console.log([...new DeclaredRange(2, 4)].join(","));
console.log([...new ExpressionRange()].join(","));
console.log(
  Object.getOwnPropertySymbols(ExpressionRange.prototype).map(String).join(","),
  Object.getOwnPropertyNames(ExpressionRange.prototype).join(","),
);

const own: any = new DeclaredRange(1, 2);
own[Symbol.iterator] = function* () {
  yield 99;
};
console.log([...own].join(","));

(DeclaredRange.prototype as any)[Symbol.iterator] = function* () {
  yield 70;
  yield 71;
};
console.log([...new DeclaredRange(1, 2)].join(","));

const iterator: any = new Map([[1, "a"], [2, "b"]]).entries();
let closed = 0;
iterator.return = () => {
  closed++;
  return { done: true, value: undefined };
};
for (const _entry of iterator) break;
console.log("closed", closed);
