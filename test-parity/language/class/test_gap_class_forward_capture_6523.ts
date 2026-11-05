// #6523: class members referencing a lexical binding (`const`/`let`) declared
// AFTER the class. Legal JS — TDZ applies at method-CALL time, and the
// bindings are initialized before any member runs — but Perry's class-capture
// machinery broke this two ways:
//
// A. Constructor-only references were invisible to the forward-capture
//    pre-pass (`cic_class` had no Constructor arm), so the binding never got
//    a box, `collect_method_captures` dropped it, and the reference fell
//    through to the global lookup — "a is not defined" at `new` time.
// B. When the binding WAS pre-registered (also referenced by a method or
//    closure), the #6465 `ClassExprFresh` decl-site capture snapshot did a
//    CHECKED TDZ box read with no suppression window, so merely DEFINING the
//    class threw "Cannot access ... before initialization" (bundled semver's
//    `Comparator` — a Next.js standalone server died at boot).
//
// The observable is byte-for-byte identical to `node --experimental-strip-types`.

// --- A: forward consts referenced ONLY from the constructor -----------------
(function factoryA(e: any) {
  const s = Symbol("x");
  class Comparator {
    max: number;
    static get ANY() {
      return s;
    }
    constructor(v: string) {
      a("dbg", v);
      this.max = n;
    }
  }
  e.exports = Comparator;
  const a = function (tag: string, v: string) {
    console.log("A: debug", tag, v);
  };
  const n = 16;
  const c = new Comparator("x");
  console.log("A: max =", c.max);
  console.log("A: ANY =", String(Comparator.ANY));
})({ exports: {} });

// --- B: the bundled-semver shape — methods + ctor + static getter reference
// --- a debug fn and sibling "require" consts declared BELOW the class -------
(function factoryB(module_: any) {
  const s = Symbol("SemVer ANY");
  class Comparator {
    value: string = "";
    semver: any;
    static get ANY() {
      return s;
    }
    constructor(comp: string) {
      debug("comparator", comp);
      this.parse(comp);
      if (this.semver === s) {
        this.value = "";
      } else {
        this.value = this.operator + this.semver;
      }
      debug("comp", this.value);
    }
    operator: string = "";
    parse(comp: string) {
      const m = comp.match(re[COMPARATOR]);
      if (!m) {
        this.semver = s;
        return;
      }
      this.operator = m[1] !== undefined ? m[1] : "";
      this.semver = m[2] !== undefined ? m[2] : s;
    }
    test(version: string) {
      debug("Comparator.test", version, this.value);
      return this.semver === s || version === this.semver;
    }
  }
  module_.exports = Comparator;
  const debug = (...args: any[]) => {
    console.log("B: debug:", ...args);
  };
  const re: RegExp[] = [];
  const COMPARATOR = 0;
  re[COMPARATOR] = /^(>=|<=|>|<|=)?(.+)$/;

  // Same-module construction (static `new` path, live cap args).
  const gte = new Comparator(">=1.2.3");
  console.log("B: operator =", gte.operator, "semver =", gte.semver);
  console.log("B: test =", gte.test("1.2.3"));
  // Construction through the escaped class VALUE (dynamic path — replays the
  // ctor with the decl-site/refreshed capture environment).
  const Exported = module_.exports;
  const any = new Exported("");
  console.log("B: any.value =", JSON.stringify(any.value));
  console.log("B: any.test =", any.test("9.9.9"));
  console.log("B: ANY is s =", Exported.ANY === s);
})({ exports: {} });

// --- regression guard: capture declared BEFORE the class, reassigned after —
// --- the refresh must keep tracking (pre-existing behavior) -----------------
(function factoryC() {
  let x = 1;
  class C {
    m() {
      return x;
    }
  }
  x = 2;
  console.log("C: m =", new C().m());
})();

// --- regression guard: plain closures capturing forward consts keep working -
(function factoryD() {
  const f = () => k * 2;
  const k = 21;
  console.log("D: f =", f());
})();
