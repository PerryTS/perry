// A throw that crosses frames: the handler is in a caller, one or more frames
// up, possibly with runtime frames (callbacks of built-ins) in between. Each
// case prints what a handler saw and the state the abandoned frames left.

const log: string[] = [];
function out(s: string): void {
  console.log(s);
}

class Boom extends Error {
  tag: string;
  constructor(tag: string) {
    super("boom " + tag);
    this.tag = tag;
    this.name = "Boom";
  }
}

// 1. Through one, two and ten frames.
function thrower(tag: string): never {
  throw new Boom(tag);
}
function one(tag: string): number {
  thrower(tag);
}
function two(tag: string): number {
  return one(tag) + 1;
}
function deep(n: number, tag: string): number {
  if (n === 0) thrower(tag);
  return deep(n - 1, tag) + 1;
}
for (const [name, f] of [
  ["one", () => one("1")],
  ["two", () => two("2")],
  ["ten", () => deep(10, "10")],
] as const) {
  try {
    f();
    out(name + ": no throw");
  } catch (e) {
    out(name + ": " + (e instanceof Boom) + " " + (e as Boom).tag + " " + (e as Error).message);
  }
}

// 2. Through a frame with a finally (and a finally that runs in each frame).
function withFinally(depth: number): number {
  try {
    if (depth === 0) throw new Boom("fin");
    return withFinally(depth - 1);
  } finally {
    log.push("finally " + depth);
  }
}
try {
  withFinally(3);
} catch (e) {
  out("finally chain: " + (e as Boom).tag + " " + log.join(","));
}
log.length = 0;

// A finally that replaces the exception on the way out.
function replacing(): number {
  try {
    thrower("inner");
  } finally {
    // eslint-disable-next-line no-unsafe-finally
    throw new Boom("replaced");
  }
}
try {
  replacing();
} catch (e) {
  out("replaced: " + (e as Boom).tag);
}

// 3. Through runtime frames: callbacks of built-ins.
try {
  [1, 2, 3].map((x) => {
    if (x === 2) thrower("map");
    return x;
  });
} catch (e) {
  out("map: " + (e as Boom).tag);
}
try {
  [3, 1, 2].sort((a, b) => {
    if (a === 1 || b === 1) thrower("sort");
    return a - b;
  });
} catch (e) {
  out("sort: " + (e as Boom).tag);
}
try {
  JSON.parse('{"a":1,"b":{"c":2}}', (k, v) => {
    if (k === "c") thrower("reviver");
    return v;
  });
} catch (e) {
  out("reviver: " + (e as Boom).tag);
}
const withGetter = {
  get value(): number {
    return one("getter");
  },
};
try {
  out("getter value " + withGetter.value);
} catch (e) {
  out("getter: " + (e as Boom).tag);
}
const m = new Map<number, number>([[1, 1], [2, 2], [3, 3]]);
let seen = 0;
try {
  m.forEach((v) => {
    seen++;
    if (v === 2) thrower("map.forEach");
  });
} catch (e) {
  out("Map.forEach: " + (e as Boom).tag + " seen " + seen);
}
// The map is usable after an abandoned forEach (deletes, then a full walk).
m.delete(1);
m.set(4, 4);
const keys: number[] = [];
m.forEach((_v, k) => keys.push(k));
out("Map after: " + keys.join(","));
const s = new Set<number>([1, 2, 3]);
try {
  s.forEach((v) => {
    if (v === 1) thrower("set.forEach");
  });
} catch (e) {
  out("Set.forEach: " + (e as Boom).tag);
}
s.delete(2);
out("Set after: " + [...s].join(","));
// Nested: a callback that catches a throw from a deeper callback, then throws.
try {
  [1].forEach(() => {
    try {
      [2].map(() => thrower("nested-inner"));
    } catch (e) {
      out("nested caught " + (e as Boom).tag);
      thrower("nested-outer");
    }
  });
} catch (e) {
  out("nested: " + (e as Boom).tag);
}

// 4. Rethrow from a catch in an outer frame.
function catchAndRethrow(): number {
  try {
    return two("re");
  } catch (e) {
    (e as Boom).tag += "+seen";
    throw e;
  }
}
let first: unknown;
try {
  catchAndRethrow();
} catch (e) {
  first = e;
  out("rethrow: " + (e as Boom).tag);
}
try {
  try {
    throw first;
  } catch (e) {
    out("identity: " + (e === first));
    throw e;
  }
} catch (e) {
  out("identity outer: " + (e === first));
}

// Non-Error values thrown across frames.
function throwValue(v: unknown): never {
  throw v;
}
for (const v of [42, "str", null, undefined, { k: 1 }, Symbol.for("s")]) {
  try {
    (() => throwValue(v))();
  } catch (e) {
    out("value: " + (typeof e) + " " + (e === v));
  }
}

// 5. A throw in a generator resumed from another frame.
function* gen(): Generator<number, void, number> {
  try {
    const x = yield 1;
    out("gen got " + x);
    yield 2;
    one("gen-body");
  } finally {
    out("gen finally");
  }
}
const g = gen();
g.next();
function resume(v: number): IteratorResult<number, void> {
  return g.next(v);
}
resume(7);
try {
  resume(8);
} catch (e) {
  out("generator: " + (e as Boom).tag);
}
out("generator after: " + JSON.stringify(g.next()));
function* gen2(): Generator<number> {
  try {
    yield 1;
  } catch (e) {
    out("gen2 caught " + (e as Boom).tag);
    yield 2;
  }
}
const g2 = gen2();
g2.next();
function throwInto(): IteratorResult<number> {
  return g2.throw(new Boom("into"));
}
out("gen2 throw -> " + JSON.stringify(throwInto()));

// 6. Async chain of three awaits; 7. a throw during module init caught by an
// importing module; 8. uncaughtException order.
async function a3(): Promise<number> {
  await null;
  thrower("async");
}
async function a2(): Promise<number> {
  const v = await a3();
  return v + 1;
}
async function a1(): Promise<number> {
  try {
    return await a2();
  } finally {
    out("a1 finally");
  }
}

async function main(): Promise<void> {
  try {
    await a1();
  } catch (e) {
    out("async chain: " + (e as Boom).tag);
  }
  try {
    await import("./fixtures/throw-across-frames-init.ts");
    out("import: no throw");
  } catch (e) {
    out("import: " + (e as Error).message);
  }
  process.on("uncaughtException", (e) => {
    out("uncaught: " + (e as Boom).tag);
  });
  setTimeout(() => {
    out("timer 1");
    one("uncaught-timer");
  }, 0);
  setTimeout(() => out("timer 2 runs after the uncaught throw"), 5);
  out("main end");
}
main();
