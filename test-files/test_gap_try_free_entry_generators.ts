// ehfree slice 1 (design §5 tests 4 and 5): with no handler recorded at
// `try` entry, a throw across `yield`/`await` inside try/finally, `return()`
// and `throw()` on a suspended generator, and re-entrant throws (a catch that
// throws, a catch calling code that catches internally, a throw from an
// iterator's `return()` during a cleanup pad) must land exactly where node
// lands them, with every finally run once.

// (A finally around the suspended `yield`/`await` itself is skipped by an
// abrupt resume on main too — a generator-transform gap tracked separately;
// these cases keep every finally outside the suspension point.)
function* g(): Generator<number, string, unknown> {
  try {
    yield 1;
    try {
      console.log("4 inner body");
    } finally {
      console.log("4 inner finally");
    }
    yield 2;
    yield 3;
  } catch (e) {
    console.log("4 generator caught:", (e as Error).message);
    yield 99;
  } finally {
    console.log("4 outer finally");
  }
  return "done";
}

{
  const it = g();
  console.log("4", JSON.stringify(it.next()));
  console.log("4", JSON.stringify(it.next()));
  console.log("4 throw()", JSON.stringify(it.throw(new Error("injected"))));
  console.log("4", JSON.stringify(it.next()));
}
{
  const it = g();
  it.next();
  it.next();
  console.log("4 return()", JSON.stringify(it.return("early")));
  console.log("4", JSON.stringify(it.next()));
}
{
  const it = g();
  try {
    it.throw(new Error("before start"));
  } catch (e) {
    console.log("4 unstarted throw:", (e as Error).message);
  }
}

async function asyncSteps(): Promise<string> {
  const order: string[] = [];
  try {
    order.push("a");
    await null;
    try {
      order.push("inner body");
    } finally {
      order.push("inner finally");
    }
    await Promise.reject(new Error("rejected"));
  } catch (e) {
    order.push("caught " + (e as Error).message);
    await null;
  } finally {
    order.push("outer finally");
  }
  return order.join(",");
}

// 5. Re-entrant throws.
function catchThrows(): string {
  try {
    try {
      throw new Error("one");
    } catch (e) {
      throw new Error("two after " + (e as Error).message);
    }
  } catch (e) {
    return (e as Error).message;
  }
}

function catchesInternally(): string {
  try {
    return "inner ok";
  } finally {
    try {
      JSON.parse("{nope");
    } catch {
      // swallowed inside a finally of a returning try
    }
  }
}

function catchCallsCatcher(): string {
  try {
    throw new Error("outer");
  } catch (e) {
    const inner = catchesInternally();
    return (e as Error).message + " / " + inner;
  }
}

class Closer implements Iterable<number> {
  closed = 0;
  [Symbol.iterator](): Iterator<number> {
    let i = 0;
    return {
      next: () => ({ value: i++, done: i > 5 }),
      return: () => {
        this.closed++;
        throw new Error("return() threw");
      },
    };
  }
}

function iteratorReturnThrows(): string {
  const c = new Closer();
  try {
    for (const v of c) {
      if (v === 2) throw new Error("body threw");
    }
  } catch (e) {
    // The body's exception wins over the one from return() (spec
    // IteratorClose with a throw completion).
    return (e as Error).message + " closed=" + c.closed;
  }
  return "WRONG";
}

function iteratorReturnThrowsOnBreak(): string {
  const c = new Closer();
  try {
    for (const v of c) {
      if (v === 1) break;
    }
  } catch (e) {
    return (e as Error).message + " closed=" + c.closed;
  }
  return "WRONG";
}

console.log("5", catchThrows());
console.log("5", catchCallsCatcher());
console.log("5", iteratorReturnThrows());
console.log("5", iteratorReturnThrowsOnBreak());

// A generator driven by a runtime trap (Array.from) whose body catches.
function* catchesInside(): Generator<number> {
  for (let i = 0; i < 3; i++) {
    try {
      if (i === 1) throw new Error("mid");
      yield i;
    } catch {
      yield -1;
    }
  }
}
console.log("4 Array.from", Array.from(catchesInside()).join(","));

asyncSteps().then((r) => console.log("4 async", r));
