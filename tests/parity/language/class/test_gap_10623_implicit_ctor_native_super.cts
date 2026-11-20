// #10623: a derived class with NO explicit constructor does not forward its
// `new`-site arguments to a native base's `super(...)`.
//
//   const { AsyncResource } = require("node:async_hooks");
//   class NoCtor extends AsyncResource {}
//   new NoCtor("MyResource");   // threw: "type" argument must be of type string
//
// Root cause: class-heritage resolution treats ANY in-scope local binding
// with the same name as the parent as "the user shadowed the native base with
// their own value" (`locally_shadowed` in `perry-hir/src/lower_decl/
// class_decl.rs`), which routes `super()` through a generic call-the-value
// dispatch instead of the native base's real init (`js_async_resource_
// subclass_init` and friends). That heuristic is right for a GENUINE shadow
// (`const EventEmitter = MyOwnClass; class X extends EventEmitter {}`), but a
// CJS-wrapped module (this file) runs its ENTIRE body inside the wrap's IIFE,
// so `const { AsyncResource } = require("node:async_hooks")` is *also* a
// genuine local — indistinguishable from real shadowing under the old check.
// AsyncResource's runtime value is a real ES `class`, so the fallback dispatch
// (calling it without `new`) threw "Class constructor AsyncResource cannot be
// invoked without 'new'" — for BOTH the implicit AND the explicit `super(type)`
// form (this file's CJS/CommonJS shape is what real npm packages use; the
// issue's "explicit works" observation held only for an ESM-module variant of
// the same source, not for this one).
//
// The fix distinguishes the two by PROVENANCE instead of by re-deriving the
// name: a local is not "shadowing" if it was ALSO destructured from a
// `require()` of the real native module with a matching export key.

const { AsyncResource, AsyncLocalStorage } = require("node:async_hooks");
const { EventEmitter, EventEmitterAsyncResource } = require("node:events");
const { Readable } = require("node:stream");

function run(label: string, fn: () => void) {
  try {
    fn();
    console.log(label, "ok");
  } catch (e: any) {
    console.log(label, "threw:", e.constructor.name + ":", e.message);
  }
}

// ── the issue's exact repro: no ctor, no override ──
run("AsyncResource implicit", () => {
  class NoCtor extends AsyncResource {}
  const a = new NoCtor("MyResource");
  console.log("  ", a.constructor.name, typeof a.triggerAsyncId, a instanceof AsyncResource);
});

// ── explicit-ctor control: this form must keep working ──
run("AsyncResource explicit", () => {
  class WithCtor extends AsyncResource {
    constructor(type: string) {
      super(type);
    }
  }
  const b = new WithCtor("MyResource2");
  console.log("  ", b.constructor.name, typeof b.triggerAsyncId, b instanceof AsyncResource);
});

// ── two-level (indirect) subclass, no constructor anywhere ──
run("AsyncResource two-level", () => {
  class Mid extends AsyncResource {}
  class Leaf extends Mid {}
  const l = new Leaf("LeafResource");
  console.log("  ", l.constructor.name, typeof l.triggerAsyncId, l instanceof AsyncResource);
});

// ── class EXPRESSION, constructor-less ──
run("AsyncResource class-expr", () => {
  const Anon = class extends AsyncResource {};
  const inst = new Anon("AnonResource");
  console.log("  ", inst.constructor.name, typeof inst.triggerAsyncId, inst instanceof AsyncResource);
});

// ── other native bases reached the SAME way (require() destructure), same
//    class-of-defect coverage: constructor-less + explicit-ctor control ──
// Note: this checks the construction surface only, not `instanceof
// AsyncLocalStorage` — that comparison has its own pre-existing gap
// (unrelated to #10623: it reproduces identically whether or not this class
// forwards constructor arguments, and #10623's fix does not touch
// `instanceof` resolution) filed separately.
run("AsyncLocalStorage implicit", () => {
  class NoCtorALS extends AsyncLocalStorage {}
  const s = new NoCtorALS();
  console.log("  ", typeof s.run, typeof s.getStore);
});

// Same `instanceof`-only carve-out as the AsyncLocalStorage case above.
run("EventEmitterAsyncResource implicit", () => {
  class NoCtorEEAR extends EventEmitterAsyncResource {}
  const e = new NoCtorEEAR();
  console.log("  ", typeof e.on, typeof e.triggerAsyncId);
});

run("EventEmitter implicit", () => {
  class NoCtorEE extends EventEmitter {}
  const ee = new NoCtorEE();
  let got = 0;
  ee.on("ping", (v: number) => (got = v));
  ee.emit("ping", 7);
  console.log("  ", typeof ee.on, got);
});

run("EventEmitter explicit", () => {
  class WithCtorEE extends EventEmitter {
    tag: string;
    constructor(tag: string) {
      super();
      this.tag = tag;
    }
  }
  const ee = new WithCtorEE("t1");
  console.log("  ", typeof ee.on, ee.tag);
});

run("Readable implicit", () => {
  class NoCtorR extends Readable {}
  const r = new NoCtorR({ read() {} });
  console.log("  ", typeof r.push, typeof r.pipe);
});

run("Readable explicit", () => {
  class WithCtorR extends Readable {
    constructor(opts: any) {
      super(opts);
    }
  }
  const r = new WithCtorR({ read() {} });
  console.log("  ", typeof r.push);
});

// ── Error family: a DIFFERENT (already-correct) mechanism; kept as a
//    same-file control so a future regression here shows up next to #10623 ──
run("Error implicit", () => {
  class NoCtorErr extends Error {}
  const e = new NoCtorErr("boom");
  console.log("  ", e.message, e instanceof Error);
});

run("TypeError implicit", () => {
  class NoCtorTErr extends TypeError {}
  const e = new NoCtorTErr("bad type");
  console.log("  ", e.message, e instanceof TypeError);
});
