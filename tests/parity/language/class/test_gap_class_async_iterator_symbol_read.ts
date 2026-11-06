// A class's `[Symbol.asyncIterator]()` method lives on the prototype under the
// symbol: `obj[Symbol.asyncIterator]` reads it (React Flight serializes a value
// as an async iterable when `typeof value[Symbol.asyncIterator] === "function"`;
// Next.js's VaryParamsAccumulator failed that test and was rejected as a class
// instance), and no string key "@@asyncIterator" appears on the prototype.
const k = Symbol.asyncIterator;

class Acc {
  _buf: string[] = [];
  _done = false;
  add(x: string) { this._buf.push(x); }
  close() { this._done = true; }
  [Symbol.asyncIterator]() {
    return {
      next: () =>
        this._buf.length > 0
          ? Promise.resolve({ value: this._buf.shift(), done: false })
          : Promise.resolve({ value: undefined, done: true }),
    };
  }
}

class Gen {
  async *[Symbol.asyncIterator]() {
    yield 1;
    yield 2;
  }
}

class SubAcc extends Acc {}

const Expr = class {
  [Symbol.asyncIterator]() {
    let n = 0;
    return { next: async () => (n < 2 ? { value: "e" + n++, done: false } : { value: undefined, done: true }) };
  }
};

const a = new Acc();
a.add("x");
a.add("y");
a.close();
console.log(typeof a[Symbol.asyncIterator], typeof (a as any)[k], k in a);
console.log(Object.getOwnPropertyNames(Acc.prototype).join(","), Object.getOwnPropertySymbols(Acc.prototype).length);
console.log(typeof new Gen()[Symbol.asyncIterator], typeof new SubAcc()[k], typeof new Expr()[k]);
console.log(Object.getOwnPropertyNames(Gen.prototype).join(","));

(async () => {
  for await (const v of a) console.log("acc", v);
  for await (const v of new Gen()) console.log("gen", v);
  const s = new SubAcc();
  s.add("s");
  for await (const v of s) console.log("sub", v);
  for await (const v of new Expr()) console.log("expr", v);
  const it = (a as any)[k]();
  console.log("manual", JSON.stringify(await it.next()));
})();
