// #1845 / #321: a *nested arrow* that captures `this` inside a COMPUTED-KEY
// method (`[KEY]() { ... () => this ... }`) snapshotted a bogus `0.0` sentinel
// instead of the receiver, so the arrow's `this` arrived as the number `0`.
//
// Computed-key methods lower to a per-instance function-expression closure
// field (see `lower_computed_key_method_as_field`): their `this` is bound
// DYNAMICALLY at call time via the runtime's `IMPLICIT_THIS`, so the enclosing
// codegen frame has an empty `this_stack`. A direct `this` read in such a body
// already falls back to `js_implicit_this_get`, but the nested-closure
// `captures_this` patch site used the `0.0` sentinel when `this_stack` was
// empty. Fix: fall back to `js_implicit_this_get` there too, matching the
// direct-read path.
//
// This was effect's `Effect.all` / `Effect.forEach` blocker: the fiber-runtime
// handler `[OP_WITH_RUNTIME](op) { internalCall(() => op.i0(this, ...)) }`
// passed `this = 0` into the WithRuntime body, so `fiber.currentContext` read
// `undefined` and the fiber died with a `{}` FiberFailure.
//
// Compared byte-for-byte against `node --experimental-strip-types`.

const wrap = <A>(body: () => A): A => body();
const KEY = "run";

function readCtx(fiber: any): any {
  return fiber && fiber.ctx;
}

class Runtime {
  public ctx: { v: number } = { v: 42 };

  // Computed-key method: nested arrow captures `this` and passes it as a
  // call argument to another function (effect's WithRuntime shape).
  [KEY](): any {
    return wrap(() => readCtx(this));
  }

  // Plain method with the identical body must keep working.
  plain(): any {
    return wrap(() => readCtx(this));
  }
}

const r: any = new Runtime();
console.log("computed-key:", JSON.stringify(r[KEY]())); // {"v":42}
console.log("plain:", JSON.stringify(r.plain())); // {"v":42}

// Nested arrow reads `this.field` directly (not just passing `this`).
class Counter {
  public n = 7;
  ["bump"](by: number): number {
    return wrap(() => this.n + by);
  }
}
const c: any = new Counter();
console.log("nested this.field:", c["bump"](3)); // 10

// Two levels of nested arrow, both capturing `this`, in a computed-key method.
class Deep {
  public label = "deep";
  ["go"](): string {
    return wrap(() => wrap(() => this.label + "!"));
  }
}
const d: any = new Deep();
console.log("double-nested:", d["go"]()); // deep!

// Symbol-keyed method whose nested arrow captures `this` (effect's
// `[Hash.symbol]()` / `[Equal.symbol]()` shape).
const TAG = Symbol("tag");
class WithSym {
  public val = 99;
  [TAG](): number {
    return wrap(() => this.val);
  }
}
const ws: any = new WithSym();
console.log("symbol-key nested this:", ws[TAG]()); // 99
