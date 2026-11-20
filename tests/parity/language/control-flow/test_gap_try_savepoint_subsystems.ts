// Every `try` records a savepoint of each runtime stack a throw can abandon
// (forEach walks, prototype resolution, private brands, derived super
// bindings, interpreter roots, pump depth). Stacks no program has used yet are
// captured as their idle value without a thread-local read, so each block
// below first catches a throw BEFORE the subsystem is ever touched, then
// throws out of the subsystem and checks it recovered.

function probe(label: string, f: () => unknown): void {
  try {
    const r = f();
    console.log(label, "ok", r);
  } catch (e) {
    console.log(label, "caught", e instanceof Error ? e.message : String(e));
  }
}

// A try that sees every subsystem idle.
probe("idle", () => {
  throw new Error("before any subsystem");
});

// Set.forEach: throw mid-walk, then delete during a later walk (compaction
// must not stay inhibited by the abandoned walk).
const set = new Set<number>([1, 2, 3, 4, 5, 6, 7, 8]);
probe("set.forEach", () => {
  set.forEach((v) => {
    if (v === 3) throw new Error("set walk " + v);
  });
});
for (let round = 0; round < 3; round++) {
  const seen: number[] = [];
  set.forEach((v) => {
    seen.push(v);
    if (v % 2 === 0) set.delete(v);
  });
  set.add(100 + round);
  console.log("set round", round, seen.join(","), set.size);
}

// Map.forEach, nested inside a try that is itself inside a walk.
const map = new Map<string, number>([["a", 1], ["b", 2], ["c", 3]]);
probe("map.forEach nested", () => {
  map.forEach((v, k) => {
    try {
      map.forEach((w) => {
        if (w === 2) throw new Error("inner " + k);
      });
    } catch (e) {
      if (k === "c") throw e;
    }
  });
});
map.delete("b");
map.set("d", 4);
const mapSeen: string[] = [];
map.forEach((v, k) => mapSeen.push(k + v));
console.log("map after", mapSeen.join(","));

// Prototype resolution through a throwing inherited getter.
class Base {
  get boom(): number {
    throw new Error("getter throws");
  }
  get fine(): number {
    return 42;
  }
}
class Derived extends Base {}
const d = new Derived();
for (let i = 0; i < 3; i++) {
  probe("proto getter " + i, () => (d as any).boom);
}
console.log("proto fine", d.fine);

// Private brands and static private methods.
class Secret {
  #value = 7;
  static #tag(): string {
    return "static-private";
  }
  #check(): number {
    if (this.#value > 5) throw new Error("private method throws");
    return this.#value;
  }
  run(): number {
    return this.#check();
  }
  static tag(): string {
    return Secret.#tag();
  }
  lower(): void {
    this.#value = 1;
  }
}
const secret = new Secret();
probe("private throw", () => secret.run());
secret.lower();
probe("private ok", () => secret.run());
console.log("static private", Secret.tag());

// Derived constructor binding `this` through an arrow, throwing after super().
class Parent {
  tag: string;
  constructor() {
    this.tag = "parent";
  }
}
class Child extends Parent {
  constructor(fail: boolean) {
    const bind = () => super();
    bind();
    if (fail) throw new Error("after super");
  }
}
probe("derived throw", () => new Child(true).tag);
probe("derived ok", () => new Child(false).tag);

// eval: interpreter roots and call depth abandoned by a throw.
probe("eval throw", () => eval("(function f(n) { if (n === 0) throw new Error('deep eval'); return f(n - 1); })(20)"));
probe("eval ok", () => eval("[1, 2, 3].map(function (x) { return x * 2; }).join('-')"));

// Regex literal construction inside a factory that throws.
function makeMatcher(fail: boolean): RegExp {
  const re = /ab+c/g;
  if (fail) throw new Error("regex factory throws");
  return re;
}
probe("regex throw", () => makeMatcher(true));
probe("regex ok", () => makeMatcher(false).test("xabbbc"));

// finally, nested try depth, and generators.
function* gen(): Generator<number> {
  try {
    yield 1;
    throw new Error("generator throws");
  } finally {
    console.log("generator finally");
  }
}
probe("generator", () => {
  const out: number[] = [];
  for (const v of gen()) out.push(v);
  return out.join(",");
});
let depth = 0;
function nest(n: number): number {
  try {
    depth++;
    if (n === 0) throw new Error("bottom");
    return nest(n - 1);
  } finally {
    depth--;
  }
}
probe("nested", () => nest(50));
console.log("depth restored", depth);

// Async: rejection across awaits and the event-loop pump.
async function asyncThrow(n: number): Promise<number> {
  await null;
  if (n > 2) throw new Error("async " + n);
  return n;
}
(async () => {
  for (let i = 0; i < 5; i++) {
    try {
      console.log("async value", await asyncThrow(i));
    } catch (e) {
      console.log("async caught", (e as Error).message);
    }
  }
  setTimeout(() => {
    try {
      throw new Error("timer throws");
    } catch (e) {
      console.log("timer caught", (e as Error).message);
    }
  }, 0);
})();
