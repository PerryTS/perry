// #10524: array destructuring of a source with NO static array proof (an `any`,
// an untyped call result — i.e. almost everything in compiled JavaScript) reads
// the elements by index when the runtime proves the value is an ordinary Array
// whose iteration nobody can observe, and drives the full iterator protocol for
// everything else. Every observable behaviour of the protocol has to survive:
// holes, defaults, a length that shrinks mid-pattern, non-array iterables,
// Array subclasses, Proxies, index accessors, prototype-inherited elements,
// and a patched prototype.

const out: string[] = [];
function log(label: string, value: unknown): void {
  out.push(label + "=" + String(value));
}
function untyped(v: unknown): any {
  return v;
}

// --- the measured shapes ----------------------------------------------------
const pairAny: any = [3, 4];
const [pa, pb] = pairAny;
log("any.pair", pa + ":" + pb);
function mapped(i: number): any {
  return [i, i + 1].map((x) => x * 2);
}
const [ma, mb] = mapped(5);
log("call.pair", ma + ":" + mb);
let sum = 0;
for (let i = 0; i < 2000; i++) {
  const [x, y] = mapped(i);
  sum += y - x;
}
log("call.loop", sum);

// Assignment form.
let asA: any = 0;
let asB: any = 0;
[asA, asB] = untyped(["p", "q"]);
log("assign.pair", asA + ":" + asB);
[asA, asB] = [asB, asA];
log("assign.swap", asA + ":" + asB);

// --- holes, short sources, defaults ------------------------------------------
const holey = untyped([1, , 3]);
const [h0, h1, h2, h3] = holey;
log("hole.values", h0 + ":" + String(h1) + ":" + h2 + ":" + String(h3));
const [hd0 = "d0", hd1 = "d1"] = untyped([, 5]);
log("hole.defaults", hd0 + ":" + hd1);
const lengthOnly: any = [];
lengthOnly.length = 2;
const [lo0 = "x", lo1 = "y"] = lengthOnly;
log("length.only", lo0 + ":" + lo1);
const [s0, s1 = "s1", s2] = untyped([0]);
log("short", s0 + ":" + s1 + ":" + String(s2));
const [nanKept = 99] = untyped([NaN]);
log("default.nan", nanKept);
const [undefFires = 42] = untyped([undefined]);
log("default.undefined", undefFires);
const [nullKept = 1] = untyped([null]);
log("default.null", nullKept);
// The iterator re-reads `length` per step, so a truncation inside a default is
// visible to the next element.
const shrink: any = [undefined, 2, 3];
const [k0 = ((shrink.length = 1), "k0"), k1 = "k1", k2 = "k2"] = shrink;
log("shrink", k0 + ":" + k1 + ":" + k2);
// A default that grows the array is visible too.
const grow: any = [undefined];
const [g0 = (grow.push("pushed"), "g0"), g1] = grow;
log("grow", g0 + ":" + g1);

// A grown (reallocated) array: the element read must follow the new storage.
const grown: any = [];
for (let i = 0; i < 100; i++) grown.push(i * 3);
const [gr0, gr1] = grown;
log("grown", gr0 + ":" + gr1 + ":" + grown.length);

// Mixed element kinds.
const [mx0, mx1, mx2, mx3] = untyped(["s", 1.5, { k: 1 }, [2]]);
log("mixed", mx0 + ":" + mx1 + ":" + mx2.k + ":" + mx3[0]);

// Nested patterns inside an untyped top-level source.
const [[n0], { n1 }, [n2 = "n2"] = []] = untyped([[7], { n1: 8 }]);
log("nested", n0 + ":" + n1 + ":" + n2);

// Elision-only pattern.
const [, , third] = untyped([1, 2, 3]);
log("elision", third);

// --- non-arrays still take the protocol --------------------------------------
const [strA, strB] = untyped("h\u{1F600}");
log("string", strA + ":" + strB.length);
const [setA, setB] = untyped(new Set([4, 5, 6]));
log("set", setA + ":" + setB);
const [[mapK, mapV]] = untyped(new Map([["k", 1]]));
log("map", mapK + ":" + mapV);
function* gen(): Generator<number> {
  yield 1;
  yield 2;
  yield 3;
}
const [genA, genB] = untyped(gen());
log("gen", genA + ":" + genB);
const [ta0, ta1] = untyped(new Uint8Array([9, 8]));
log("typedarray", ta0 + ":" + ta1);
function argsCase(..._xs: number[]): string {
  const [x, y] = untyped(arguments);
  return x + ":" + y;
}
log("arguments", argsCase(6, 7));

let customNexts = 0;
let customReturns = 0;
const custom: any = {
  [Symbol.iterator]() {
    let i = 0;
    return {
      next() {
        customNexts++;
        i++;
        return { done: false, value: i * 10 };
      },
      return() {
        customReturns++;
        return { done: true };
      },
    };
  },
};
const [c0, c1] = custom;
log("custom", c0 + ":" + c1 + ":" + customNexts + ":" + customReturns);

for (const bad of [undefined, null, 42, {}]) {
  try {
    const [z] = untyped(bad);
    log("bad", "no throw " + String(z));
  } catch (e) {
    log("bad", e instanceof TypeError);
  }
}

// --- arrays whose iteration is observable take the protocol -----------------
class Doubled extends Array<number> {
  *[Symbol.iterator](): IterableIterator<number> {
    for (let i = 0; i < this.length; i++) yield this[i] * 2;
  }
}
const dbl = new Doubled();
dbl.push(1, 2);
const [db0, db1] = untyped(dbl);
log("subclass.iter", db0 + ":" + db1);
class Plain extends Array<number> {}
const plainSub = new Plain();
plainSub.push(4, 5);
const [ps0, ps1] = untyped(plainSub);
log("subclass.plain", ps0 + ":" + ps1);

const trapLog: string[] = [];
const proxied: any = new Proxy([1, 2], {
  get(target: any, key: any, recv: any) {
    if (typeof key === "string") trapLog.push(key);
    return Reflect.get(target, key, recv);
  },
});
const [px0, px1] = proxied;
log("proxy", px0 + ":" + px1 + ":" + trapLog.join(","));

let getterCalls = 0;
const accessor: any = [0, 2];
Object.defineProperty(accessor, 0, {
  get() {
    getterCalls++;
    return "got";
  },
});
const [ac0, ac1] = accessor;
log("accessor", ac0 + ":" + ac1 + ":" + getterCalls);

// --- sticky runtime facts: run last ------------------------------------------
function inheritedElementChecks(): void {
  (Array.prototype as any)[1] = "inherited";
  const [ie0, ie1, ie2] = untyped([0, , 2]);
  log("inherited", ie0 + ":" + ie1 + ":" + ie2);
  delete (Array.prototype as any)[1];
}

function patchedPrototypeChecks(): void {
  const original = (Array.prototype as any)[Symbol.iterator];
  let patchedCalls = 0;
  (Array.prototype as any)[Symbol.iterator] = function () {
    patchedCalls++;
    let i = 0;
    return {
      next: () => {
        i++;
        return { done: i > 2, value: i * 1000 };
      },
    };
  };
  const [pp0, pp1] = untyped([1, 2]);
  log("patched.any", pp0 + ":" + pp1 + ":" + patchedCalls);
  (Array.prototype as any)[Symbol.iterator] = original;
  const [rr0, rr1] = untyped([1, 2]);
  log("restored", rr0 + ":" + rr1);
}

async function asyncCase(): Promise<string> {
  const [a, b] = untyped([41, 42]);
  await Promise.resolve(0);
  let c: any = 0;
  let d: any = 0;
  [c, d] = untyped([b, a]);
  return a + ":" + b + ":" + c + ":" + d;
}

asyncCase().then((v) => {
  log("async", v);
  inheritedElementChecks();
  patchedPrototypeChecks();
  console.log(out.join("\n"));
});
