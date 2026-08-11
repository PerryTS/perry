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
