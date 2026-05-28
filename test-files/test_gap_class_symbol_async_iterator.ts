// #1838 follow-up: a class's computed `[Symbol.asyncIterator]() {}` is
// resolvable via member access. Class lowering names it `@@asyncIterator`
// in the vtable (mirroring `@@iterator`), so `instance[Symbol.asyncIterator]`,
// `Symbol.asyncIterator in instance`, and the explicit driver pattern
// `(await it.next()).value` all find it.
//
// Scope: member access / `in` / direct `await it.next()` round-trip. The
// `for await (...)` lowering on a class instance goes through a separate
// codegen path and is a follow-up. Validated byte-for-byte against Node.

class AsyncRange {
  n: number;
  constructor(n: number) {
    this.n = n;
  }
  [Symbol.asyncIterator]() {
    let i = 0;
    const n = this.n;
    return {
      next: () =>
        Promise.resolve(
          i < n ? { value: i++, done: false } : { value: undefined, done: true },
        ),
    };
  }
}

async function main(): Promise<void> {
  const r = new AsyncRange(3);
  console.log("has:", Symbol.asyncIterator in r); // true
  console.log("typeof:", typeof (r as any)[Symbol.asyncIterator]); // function

  // Direct driver: explicit `await it.next()` over the resolved iterator.
  const it = (r as any)[Symbol.asyncIterator]();
  const out: number[] = [];
  let v = await it.next();
  while (!v.done) {
    out.push(v.value);
    v = await it.next();
  }
  console.log("direct:", out.join(",")); // 0,1,2

  // Inherited [Symbol.asyncIterator] resolves through the class chain.
  class Sub extends AsyncRange {}
  const s = new Sub(2);
  console.log("inherited has:", Symbol.asyncIterator in s);
  const sit = (s as any)[Symbol.asyncIterator]();
  const sout: number[] = [];
  let sv = await sit.next();
  while (!sv.done) {
    sout.push(sv.value);
    sv = await sit.next();
  }
  console.log("inherited direct:", sout.join(",")); // 0,1

  console.log("ALL CLASS-SYMBOL-ASYNC-ITERATOR TESTS PASSED");
}

main();
