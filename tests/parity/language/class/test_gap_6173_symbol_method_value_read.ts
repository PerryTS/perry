// #6173: reading a SYMBOL-keyed class method as a VALUE must return the
// function, for both instance and static receivers. The direct call form
// (`obj[S]()`) already worked — it dispatches through
// `js_native_call_method_value`'s independent CLASS_SYMBOL_METHODS lookup —
// but the bare read went through `js_object_get_symbol_property`, which never
// consulted that table and returned `undefined`.
//
// Validated byte-for-byte against `node --experimental-strip-types`.

const S = Symbol("s");

// ── instance symbol method ──────────────────────────────────────────────────
class F {
  [S]() {
    return 99;
  }
}
const f: any = new F();
console.log(typeof f[S]); // "function"
console.log(f[S]()); // 99 (call path, regression guard)
const m = f[S];
console.log(m()); // 99 (read-then-call)

// ── static symbol method ────────────────────────────────────────────────────
class D {
  static [S]() {
    return 42;
  }
}
console.log(typeof (D as any)[S]); // "function"
console.log((D as any)[S]()); // 42 (call path, regression guard)
const sm = (D as any)[S];
console.log(sm()); // 42 (read-then-call)

// ── args and rest params flow through the bound value ───────────────────────
// (No `this` in the detached calls: Node binds `this === undefined` for a
// detached call, while Perry's bound-method values keep read-time snapshot
// semantics — same as its string-keyed method values.)
class WithArgs {
  [S](a: number, b: number, ...rest: number[]) {
    return a + b + rest.length;
  }
}
const wa: any = new WithArgs();
const wam = wa[S];
console.log(wam(1, 2)); // 3
console.log(wam(1, 2, 8, 9, 10)); // 6
console.log(wa[S].length); // 2 (rest param excluded)

// `this` binding via the direct call form (regression guard for the call path)
class WithThis {
  base = 7;
  [S](a: number) {
    return this.base + a;
  }
}
const wt: any = new WithThis();
console.log(wt[S](1)); // 8

// ── inherited through a subclass chain ──────────────────────────────────────
class Base {
  [S]() {
    return "base-inst";
  }
  static [S]() {
    return "base-static";
  }
}
class Mid extends Base {}
class Leaf extends Mid {}
const leaf: any = new Leaf();
console.log(typeof leaf[S], leaf[S]()); // function base-inst
console.log(typeof (Leaf as any)[S], (Leaf as any)[S]()); // function base-static

// ── a symbol-keyed GETTER still resolves through the accessor path ─────────
const G = Symbol("g");
class WithGetter {
  get [G]() {
    return "got";
  }
  static get [G]() {
    return "static-got";
  }
}
const wg: any = new WithGetter();
console.log(wg[G]); // "got" (getter invoked, NOT a bound method)
console.log((WithGetter as any)[G]); // "static-got"

// ── an OWN symbol property shadows the class method ─────────────────────────
const shadowed: any = new F();
shadowed[S] = 123;
console.log(shadowed[S]); // 123

// ── well-known symbol method reads keep working ─────────────────────────────
class Iter {
  *[Symbol.iterator]() {
    yield 1;
    yield 2;
  }
}
const it: any = new Iter();
console.log(typeof it[Symbol.iterator]); // "function"
console.log([...it].join(",")); // 1,2

// ── instance-side `in` shares the resolver ──────────────────────────────────
console.log(S in f); // true
console.log(G in wg); // true (accessor presence)
console.log(Symbol("other") in f); // false
