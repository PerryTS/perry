// A throw whose handler is in the throwing function, next to the same
// throws crossing frames: both must behave alike in every construct that
// sits between a throw and its handler.

class E {
  code: number;
  constructor(c: number) {
    this.code = c;
  }
}

const log: string[] = [];
function note(s: string): void {
  log.push(s);
}

// --- same frame
function sameFrame(n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) {
    try {
      if (i % 2 === 0) throw new E(i);
      s += 1000;
    } catch (e) {
      s += (e as E).code;
    }
  }
  return s;
}
note("same " + sameFrame(10));

// --- cross frame
function thrower(i: number): number {
  if (i >= 0) throw new E(i);
  return 0;
}
function crossFrame(n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) {
    try {
      s += thrower(i);
    } catch (e) {
      s += (e as E).code;
    }
  }
  return s;
}
note("cross " + crossFrame(10));

// --- primitives and undefined are thrown values like any other
function prims(): string {
  const out: string[] = [];
  const values: unknown[] = [1, "s", null, undefined, true, 0n, Symbol.iterator];
  for (const v of values) {
    try {
      throw v;
    } catch (e) {
      out.push(typeof e + ":" + (e === v));
    }
  }
  return out.join(",");
}
note("prims " + prims());

// --- finally between throw and catch, same frame
function finallyBetween(): string {
  const out: string[] = [];
  try {
    try {
      out.push("body");
      throw new E(1);
    } finally {
      out.push("finally");
    }
  } catch (e) {
    out.push("caught " + (e as E).code);
  }
  return out.join(" ");
}
note("fin " + finallyBetween());

// --- finally with catch: the catch throws, the finally runs, the outer catches
function catchThrows(): string {
  const out: string[] = [];
  try {
    try {
      throw new E(1);
    } catch (e) {
      out.push("inner " + (e as E).code);
      throw new E(2);
    } finally {
      out.push("finally");
    }
  } catch (e) {
    out.push("outer " + (e as E).code);
  }
  return out.join(" ");
}
note("catchThrows " + catchThrows());

// --- rethrow of the same value keeps its identity
function rethrow(): string {
  const original = new E(7);
  try {
    try {
      throw original;
    } catch (e) {
      throw e;
    }
  } catch (e) {
    return "rethrow " + (e === original);
  }
}
note(rethrow());

// --- a throw in finally replaces the pending one
function finallyThrows(): string {
  try {
    try {
      throw new E(1);
    } finally {
      throw new E(2);
    }
  } catch (e) {
    return "finallyThrows " + (e as E).code;
  }
}
note(finallyThrows());

// --- return, break and continue in finally drop the pending throw
function finallyReturns(): string {
  try {
    throw new E(1);
  } finally {
    return "finallyReturns ok";
  }
}
note(finallyReturns());
function finallyBreaks(): string {
  let rounds = 0;
  for (let i = 0; i < 5; i++) {
    try {
      rounds++;
      throw new E(i);
    } finally {
      if (i === 2) break;
      continue;
    }
  }
  return "finallyBreaks " + rounds;
}
note(finallyBreaks());

// --- an uncaught local throw leaves the function and runs its finally
function leaves(): number {
  try {
    throw new E(5);
  } finally {
    note("leaves finally");
  }
}
try {
  leaves();
} catch (e) {
  note("leaves caught " + (e as E).code);
}

// --- break / continue / return out of a catch
function catchControl(): string {
  const out: number[] = [];
  outer: for (let i = 0; i < 4; i++) {
    for (let j = 0; j < 4; j++) {
      try {
        if (j === 1) throw new E(j);
        out.push(i * 10 + j);
      } catch (e) {
        if (i === 0) continue outer;
        if (i === 1) break;
        if (i === 2) continue;
        return out.join(",") + " returned";
      }
    }
  }
  return out.join(",");
}
note("control " + catchControl());

// --- a constructor that throws, under a handler of the caller's frame and
// --- under a handler of its own
class Throwing {
  v: number;
  constructor(v: number) {
    if (v < 0) throw new E(v);
    this.v = v;
  }
}
class Recovering {
  v: number;
  target: boolean;
  constructor(v: number) {
    try {
      if (v < 0) throw new E(v);
      this.v = v;
    } catch (e) {
      this.v = -(e as E).code;
    }
    this.target = new.target === Recovering;
  }
}
class Derived extends Throwing {
  w: number;
  constructor(v: number) {
    super(v);
    this.w = v + 1;
  }
}
function ctors(): string {
  const out: string[] = [];
  for (const v of [1, -2]) {
    try {
      out.push("T" + new Throwing(v).v);
    } catch (e) {
      out.push("Tc" + (e as E).code);
    }
    try {
      out.push("D" + new Derived(v).w);
    } catch (e) {
      out.push("Dc" + (e as E).code);
    }
    const r = new Recovering(v);
    out.push("R" + r.v + r.target);
  }
  return out.join(" ");
}
note("ctors " + ctors());
function targetAfter(): string {
  // new.target of this frame is unaffected by a throw caught inside it.
  function Probe(this: any) {
    try {
      new Throwing(-1);
    } catch (e) {
      this.seen = new.target === Probe;
    }
  }
  return "target " + new (Probe as any)().seen;
}
note(targetAfter());

// --- for-of: a throw in the body closes the iterator, then reaches the catch
function iteratorClose(): string {
  const out: string[] = [];
  const it = {
    i: 0,
    [Symbol.iterator]() {
      return this;
    },
    next() {
      return { done: this.i >= 3, value: this.i++ };
    },
    return() {
      out.push("closed");
      return {};
    },
  };
  try {
    for (const v of it) {
      out.push("v" + v);
      if (v === 1) throw new E(v);
    }
  } catch (e) {
    out.push("caught " + (e as E).code);
  }
  return out.join(" ");
}
note("iter " + iteratorClose());

// --- the catch binding captured by a closure
function captured(): string {
  const fns: Array<() => number> = [];
  for (let i = 0; i < 3; i++) {
    try {
      throw new E(i);
    } catch (e) {
      fns.push(() => (e as E).code);
    }
  }
  return "captured " + fns.map((f) => f()).join(",");
}
note(captured());

// --- Error objects: message, name and stack head are those of the throw site
function errors(): string {
  const out: string[] = [];
  try {
    throw new TypeError("local");
  } catch (e) {
    const err = e as Error;
    out.push(err.name + ":" + err.message + ":" + (err instanceof TypeError));
    out.push(String(err.stack).split("\n")[0]);
  }
  try {
    null!.x;
  } catch (e) {
    out.push((e as Error).name);
  }
  return out.join(" ");
}
note("errors " + errors());

// --- generators: a throw inside a try that spans a yield, and gen.throw()
function* gen(): Generator<number, string, unknown> {
  let seen = "";
  try {
    yield 1;
    throw new E(2);
  } catch (e) {
    seen += "c" + (e as E).code;
  } finally {
    seen += "f";
  }
  try {
    yield 3;
  } catch (e) {
    seen += "t" + (e as E).code;
  }
  return seen;
}
function generators(): string {
  const g = gen();
  const out: string[] = [];
  out.push(String(g.next().value));
  out.push(String(g.next().value));
  out.push(String(g.throw(new E(9)).value));
  return "gen " + out.join(" ");
}
note(generators());

// --- async: a local throw under a handler, without one, and across an await
async function asyncLocal(): Promise<string> {
  try {
    throw new E(1);
  } catch (e) {
    return "asyncLocal " + (e as E).code;
  }
}
async function asyncRejects(): Promise<string> {
  throw new E(2);
}
async function asyncAwait(): Promise<string> {
  try {
    await asyncRejects();
    return "not reached";
  } catch (e) {
    try {
      await Promise.resolve();
      throw new E((e as E).code + 1);
    } catch (e2) {
      return "asyncAwait " + (e2 as E).code;
    } finally {
      note("asyncAwait finally");
    }
  }
}
async function asyncClosure(): Promise<string> {
  // A capturing closure keeps this an ordinary async body.
  const k = [3];
  const add = (n: number) => n + k[0];
  try {
    if (add(1) === 4) throw new E(add(1));
    return "not reached";
  } catch (e) {
    return "asyncClosure " + (e as E).code;
  }
}

async function main(): Promise<void> {
  note(await asyncLocal());
  note(await asyncRejects().catch((e) => "asyncRejects " + (e as E).code));
  note(await asyncAwait());
  note(await asyncClosure());
  for (const line of log) console.log(line);
}
main();
