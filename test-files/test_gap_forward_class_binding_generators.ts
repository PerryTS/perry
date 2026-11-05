// A per-evaluation class declaration (heritage is the native Error) that an
// earlier closure reads must keep one identity through the async/generator
// lowering, through generator and async function declarations, and through
// fresh class expressions that evaluate code naming it. Every line is compared
// with node. Each case runs on its own so that one failure does not hide the
// others (a case that throws prints `threw`). One class name per function:
// same-named sibling classes of one function are a separate, open defect.

async function attempt(label, fn) {
  try {
    console.log(label, await fn());
  } catch (e) {
    console.log(label, 'threw', e && e.name);
  }
}

function rec(depth) {
  const read = () => C;
  let inner;
  if (depth > 0) inner = rec(depth - 1);
  class C extends Error {}
  return { read, C, inner };
}

function scope(tag) {
  function* gen() {
    yield C;
  }
  async function asyncGet() {
    return C;
  }
  function plain() {
    return C;
  }
  class C extends Error {
    static tag = tag;
  }
  return { gen, asyncGet, plain, C };
}

function scoped() {
  const make = () =>
    class {
      static base = C;
      static [C.name] = 1;
    };
  const extend = () => class extends C {};
  class C extends Error {}
  return { make, extend, C };
}

async function main() {
  // 1. async loop: the iteration's getter is created before the declaration
  await attempt('async for', async () => {
    const results = [];
    for (let i = 0; i < 3; i++) {
      const read = () => C;
      class C extends Error {}
      await 0;
      results.push(read() === C);
    }
    return results.join(',');
  });

  // 2. for...of in an async function, closures kept past the loop
  await attempt('async for-of', async () => {
    const kept = [];
    for (const n of [1, 2, 3]) {
      const read = () => C;
      class C extends Error {
        static n = n;
      }
      await 0;
      kept.push({ read, C });
    }
    return [
      kept.map((k) => k.read() === k.C).join(','),
      kept.map((k) => k.read().n).join(','),
      kept[0].C !== kept[1].C,
    ].join(' ');
  });
  await attempt('async for static', async () => {
    const kept = [];
    for (let n = 1; n < 4; n++) {
      const read = () => C;
      class C extends Error {
        static n = n;
      }
      await 0;
      kept.push({ read, C });
    }
    return kept.map((k) => (k.read() === k.C) + ':' + k.C.n).join(',');
  });

  // 3. async arrows across an await, declaration before and after the await
  await attempt('async arrows', async () => {
    const arrowAfter = async () => {
      const read = () => C;
      class C extends Error {}
      await 0;
      return read() === C;
    };
    const arrowBefore = async () => {
      const read = () => C;
      await 0;
      class C extends Error {}
      return read() === C;
    };
    return [await arrowAfter(), await arrowBefore(), await arrowAfter()].join(' ');
  });

  // 4. a generator loop
  await attempt('generator loop', () => {
    function* loop() {
      for (let i = 0; i < 3; i++) {
        const read = () => C;
        class C extends Error {}
        yield read() === C;
      }
    }
    return [...loop()].join(',');
  });

  // 5. generator, async and plain function declarations reading a class
  // declared after them in a function scope, evaluated twice
  await attempt('declarations', async () => {
    const s1 = scope('a');
    const s2 = scope('b');
    return [
      [...s1.gen()][0] === s1.C,
      [...s2.gen()][0] === s2.C,
      (await s1.asyncGet()) === s1.C,
      (await s2.asyncGet()) === s2.C,
      s1.plain() === s1.C,
      s2.plain() === s2.C,
      s1.C !== s2.C,
      [...s1.gen()][0].tag,
      [...s2.gen()][0].tag,
    ].join(' ');
  });

  // 6. a fresh class expression evaluates its static initializers, computed
  // keys, heritage and captured values in the closure that holds it
  await attempt('class expression', () => {
    const x = scoped();
    const y = scoped();
    const m1 = x.make();
    const m2 = x.make();
    return [
      m1.base === x.C,
      m2.base === x.C,
      y.make().base === y.C,
      y.make().base !== x.C,
      m1.C,
      Object.getPrototypeOf(x.extend()) === x.C,
      Object.getPrototypeOf(y.extend()) === y.C,
      new (x.extend())() instanceof x.C,
      new (x.extend())() instanceof y.C,
    ].join(' ');
  });

  // 7. a factory re-entered before the first invocation's declaration runs
  await attempt('reentry', () => {
    const outer = rec(2);
    const mid = outer.inner;
    const inner = mid.inner;
    return [
      outer.read() === outer.C,
      mid.read() === mid.C,
      inner.read() === inner.C,
      outer.C !== mid.C,
      mid.C !== inner.C,
      outer.C !== inner.C,
    ].join(' ');
  });
}
main();
