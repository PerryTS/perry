// `proxy_locals` is a BARE-NAME set with no scope discrimination, collected by
// a module-wide pre-scan. A name bound to `new Proxy(...)` anywhere made EVERY
// function's `<name>.prop` lower to `js_proxy_get` — including functions that
// bind the same name to something else entirely, and including the case where
// the proxy's own function is never called.
//
// `js_proxy_get` on a plain array answers `undefined`, so `a.length` came back
// undefined and the `for (i = 0; i < a.length; i++)` after it ran ZERO
// iterations. Wrong answer, no diagnostic.
//
// The pre-scan already had the remedy — a `poison` set for ambiguous names —
// but it only fired for a colliding `new <OtherClass>()`, and its result was
// subtracted from `weakmap_locals`/`weakset_locals` only. Both halves are why
// this survived.

class P {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}

function build(n: number): P[] {
  const a: P[] = [];
  for (let i = 0; i < n; i++) a.push(new P(i, i + 1));
  return a;
}

// The victim: `a` here is a plain array, bound to a CALL result — the case the
// poison set did not cover.
function readLoop(): number {
  const a = build(10);
  let s = 0;
  for (let i = 0; i < a.length; i++) {
    const r = a[i];
    s += r.x + r.y;
  }
  return s;
}

// Never called. Its mere presence was enough.
function neverCalled(): number {
  const raw = build(10);
  const a: P[] = new Proxy(raw, {}) as any;
  return a.length;
}

console.log("read loop:", readLoop());
console.log("length:", build(4).length);

// The proxy need not be over the victim's array, or over any array of the same
// type — only the NAME had to collide.
function unrelatedProxyName(): number {
  const nums: number[] = [1, 2, 3];
  const a: any = new Proxy(nums, {});
  return a.length;
}
function alsoVictim(): number {
  const a = build(3);
  return a.length;
}
console.log("also victim:", alsoVictim());

// THE OTHER DIRECTION. A genuine, actually-used proxy whose name collides must
// still behave like a proxy — poisoning the name costs it a codegen fast path,
// and this asserts that the ordinary dynamic property path is still correct
// rather than merely slower. Without this the fix could "pass" by breaking
// proxies instead.
class Other {
  v = 1;
}
function realProxyWithCollidingName(): string {
  const a: any = new Proxy(
    { q: 7 },
    {
      get(target: any, key: string) {
        return key === "q" ? 42 : target[key];
      },
    },
  );
  return `${a.q} ${a.missing}`;
}
function collidingNonProxy(): number {
  const a = new Other();
  return a.v;
}
console.log("real proxy:", realProxyWithCollidingName());
console.log("colliding non-proxy:", collidingNonProxy());

// A proxy with a UNIQUE name keeps every trap working — the common case, and
// the one an over-broad poison would silently degrade.
function uniquelyNamedProxy(): string {
  const guarded: any = new Proxy(
    { hit: 0 },
    {
      get(target: any, key: string) {
        if (key === "doubled") return target.hit * 2;
        return target[key];
      },
      set(target: any, key: string, value: any) {
        target[key] = value;
        return true;
      },
    },
  );
  guarded.hit = 21;
  return `${guarded.hit} ${guarded.doubled}`;
}
console.log("unique proxy:", uniquelyNamedProxy());

// A `has` trap through a colliding name, so the fallback is exercised for more
// than a plain get.
function collidingHasTrap(): string {
  const a: any = new Proxy(
    {},
    {
      has(_target: any, key: string) {
        return key === "yes";
      },
    },
  );
  return `${"yes" in a} ${"no" in a}`;
}
console.log("colliding has trap:", collidingHasTrap());

// ---------------------------------------------------------------------------
// The shapes the poison set CANNOT reach, at all. `record_var` only matches
// `ast::Pat::Ident` declarators, so a parameter, a `for…of` head, a destructured
// binding and a catch param were never poisonable whatever their initializer;
// and an object literal / array literal / identifier copy is not a call, so the
// call-initializer arm skipped it too. Every one of these was `undefined` (or
// `NaN`, or `false`) merely because a function elsewhere — none of them called —
// spells a proxy with the same name. The fix is `is_proxy_local`, which keys on
// the RESOLVED binding, so these are the cases that must stay green.
// ---------------------------------------------------------------------------

// A plain object literal. The ten-line core of the bug.
function neverCalledObj(): number {
  const o: any = new Proxy({ v: 1 }, {});
  return o.v;
}
function readsObjectLiteral(): number {
  const o = { v: 42 };
  return o.v;
}

// A function PARAMETER — never poisonable, `record_var` never sees params.
function neverCalledParam(): number {
  const arr: any = new Proxy([1], {});
  return arr.length;
}
function readsParam(arr: number[]): number {
  return arr.length;
}

// `.length` after `.push()` on an EMPTY array literal — not a call init, so the
// #7775 poison arm did not cover it either.
function neverCalledPush(): number {
  const items: any = new Proxy([1], {});
  return items.length;
}
function pushesThenReadsLength(): number {
  const items: number[] = [];
  items.push(7);
  items.push(8);
  return items.length;
}

// A bare identifier copy.
function neverCalledCopy(): number {
  const c: any = new Proxy({ v: 1 }, {});
  return c.v;
}
function copiesIdentifier(): number {
  const src = { v: 42 };
  const c = src;
  return c.v;
}

// A `for…of` head binding.
function neverCalledForOf(): number {
  const g: any = new Proxy({ v: 1 }, {});
  return g.v;
}
function sumsForOfBinding(): number {
  let total = 0;
  for (const g of [{ v: 10 }, { v: 32 }]) total += g.v;
  return total;
}

// A destructured binding.
function neverCalledDestructure(): number {
  const d: any = new Proxy({ v: 1 }, {});
  return d.v;
}
function readsDestructured(): number {
  const box = { d: { v: 42 } };
  const { d } = box;
  return d.v;
}

// A property WRITE, and a compound write, on a same-named plain object.
function neverCalledWrite(): number {
  const w: any = new Proxy({ v: 1 }, {});
  return w.v;
}
function writesProperty(): number {
  const w = { v: 1 };
  w.v = 5;
  w.v += 1;
  return w.v;
}

// `"v" in t` on a same-named plain object.
function neverCalledIn(): boolean {
  const t: any = new Proxy({ v: 1 }, {});
  return "v" in t;
}
function checksIn(): boolean {
  const t = { v: 42 };
  return "v" in t;
}

console.log("object literal:", readsObjectLiteral());
console.log("parameter:", readsParam([1, 2, 3]));
console.log("push then length:", pushesThenReadsLength());
console.log("identifier copy:", copiesIdentifier());
console.log("for-of head:", sumsForOfBinding());
console.log("destructured:", readsDestructured());
console.log("property write:", writesProperty());
console.log("in check:", checksIn());

// ---------------------------------------------------------------------------
// The same bug pointing the other way. The pre-scan's `walk_stmt` descends into
// function DECLARATIONS only — never into a class body or an arrow body — so a
// proxy declared in a method or an arrow was never registered at all. Keying on
// the resolved binding registers it at its declarator wherever that sits, so
// these traps fire through the fast path now. Both directions have to hold at
// once: the sibling function below reuses both spellings for plain objects.
// ---------------------------------------------------------------------------
class ProxyInMethod {
  run(): string {
    const m: any = new Proxy(
      { v: 1 },
      {
        get: (t: any, k: string) => (k === "v" ? 7 : t[k]),
        set: (t: any, k: string, value: any) => {
          t[k] = value * 2;
          return true;
        },
        has: (_t: any, k: string) => k === "ghost",
      },
    );
    m.w = 5;
    return `${m.v} ${m.w} ${"ghost" in m} ${"v" in m}`;
  }
}
const proxyInArrow = (): number => {
  const r: any = new Proxy({ v: 1 }, { get: () => 8 });
  return r.v;
};
function sameNamesButPlain(): string {
  const m = { v: 41 };
  const r = { v: 40 };
  return `${m.v} ${r.v}`;
}
console.log("proxy in method:", new ProxyInMethod().run());
console.log("proxy in arrow:", proxyInArrow());
console.log("same names but plain:", sameNamesButPlain());

// A module-level proxy read from a function lowered BEFORE its declarator. The
// binding-keyed set has to be seeded during module-var pre-registration or this
// forward reference silently loses the proxy path.
function readsModuleProxyDeclaredLater(): number {
  return modProxy.v;
}
const modProxy: any = new Proxy(
  { v: 1 },
  { get: (t: any, k: string) => (k === "v" ? 64 : t[k]) },
);
console.log("forward-referenced module proxy:", readsModuleProxyDeclaredLater());
console.log("module proxy direct:", modProxy.v);
