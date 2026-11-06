// Issue #1787: `new <class-expression-value>()` must run the class
// expression's INSTANCE-field initializers — both literal fields and fields
// that capture the defining (factory) scope — and the constructor body.
//
// A class EXPRESSION with static fields lowers to a heap "class object"
// (#1786) carrying per-evaluation identity. Constructing an instance from
// such a value (`const A = mk(...); new A()`) can't inline the constructor at
// the `new` site: the callee is a runtime value, and the captured environment
// lived where the class expression was evaluated (inside `mk`), not at the
// construction site. Pre-fix the dynamic `new` allocated a bare instance with
// no own props, so every instance field read back `undefined`. Now the
// captures are snapshotted onto the class object and the constructor is
// replayed on the new instance.
//
// Scope note: per-evaluation class objects still share the compile-time
// template's class_id, so cross-evaluation `.constructor` / `instanceof`
// discrimination (telling a `mk("a")` instance apart from a `mk("b")` one) is
// the deeper shared-class_id limitation tracked separately (see the scope
// note in test_gap_class_expr_new_instanceof.ts) and is not asserted here.
//
// Expected output:
// tags: LIT LIT
// captured: a b
// computed: A-1 B-2
// static: a b
// method reads instance field: a b
// ctor arg + capture: pre:post
// no-capture literal: 7

// Captured + literal instance fields, a per-evaluation static, and an
// instance method that reads a captured instance field through `this`.
function mk(tag: string, n: number) {
  return class {
    _tag = "LIT"; // literal instance field
    cap = tag; // instance field capturing the factory arg
    computed = tag.toUpperCase() + "-" + n; // captured + computed
    static S = tag; // per-evaluation static (already worked)
    getCap() {
      return this.cap;
    }
  };
}
const A = mk("a", 1);
const B = mk("b", 2);
const xa: any = new A();
const xb: any = new B();
console.log("tags:", xa._tag, xb._tag);
console.log("captured:", xa.cap, xb.cap);
console.log("computed:", xa.computed, xb.computed);
console.log("static:", (A as any).S, (B as any).S);
console.log("method reads instance field:", xa.getCap(), xb.getCap());

// A user constructor whose body mixes a `new`-call argument with a captured
// outer value — exercises the [user params..., capture params...] ordering.
function mkWithArg(prefix: string) {
  return class {
    label = "";
    static P = prefix;
    constructor(suffix: string) {
      this.label = prefix + ":" + suffix;
    }
  };
}
const C = mkWithArg("pre");
const c: any = new C("post");
console.log("ctor arg + capture:", c.label);

// A class expression with only a literal instance field (no captures) — the
// constructor still has to run to initialize it.
function mkLiteral() {
  return class {
    v = 7;
    static kind = "lit";
  };
}
const D = mkLiteral();
const d: any = new D();
console.log("no-capture literal:", d.v);
