// #7280: the instance a RUNTIME-DISPATCHED `new` builds must be rooted across
// the constructor body, in the RUNTIME helper's frame.
//
// This is `test_gap_gc_new_instance_rooting`'s sibling one layer down. That
// one covers the codegen route: `new C(n)` with a statically-known `C`, where
// the caller holds the instance in its own SSA register/shadow slot. This one
// covers the four routes where codegen CANNOT resolve the callee and hands the
// whole construction to `js_new_function_construct` in perry-runtime:
//
//   1. `new obj.ctor(n)` where `obj.ctor` is a declared class     (ClassRef)
//   2. `new obj.ctor(n)` where `obj.ctor` is a plain function     (closure)
//   3. `new obj.ctor(n)` where `obj.ctor` is a class EXPRESSION   (class object)
//   4. `Reflect.construct(fn, [n], otherFn)`                      (newTarget)
//
// Each of those four arms allocates the instance, runs the user constructor
// body, and returns the instance — holding it in a BARE RUST LOCAL across the
// call. A runtime frame is not covered by the precise scan and the
// conservative stack scan resolves to `SkipDisabled` in shipped builds, so
// that local is an unrooted receiver: the constructor body's loop back-edge
// polls run an evacuating minor, the instance moves, the collector rewrites
// the callee's `this` but has no idea about the helper's local, and the helper
// returns the PRE-MOVE address. Every field the constructor just wrote reads
// back as garbage through it.
//
// CLAUDE.md's "known-weak areas" calls this the sibling class the static IR
// checker is structurally blind to: `scripts/gc_root_dominance_check.py` reads
// emitted LLVM IR, and none of this is in the emitted IR — it is Rust.
//
// LIVE BY CONSTRUCTION. Each constructor allocates hard enough to reach the
// collector, and each caller READS BACK a field written by the constructor
// immediately after it returns, so a stale return address is observable. A
// non-moving collection cannot expose any of this; the evacuating arms are the
// ones that bite.

class DeclaredPayload {
  n: number;
  len: number;
  constructor(n: number) {
    const bits: any[] = [];
    for (let i = 0; i < 300; i++) {
      bits.push({ i: i, s: "x" });
    }
    this.n = n;
    this.len = bits.length;
  }
}

function FunctionPayload(this: any, n: number): void {
  const bits: any[] = [];
  for (let i = 0; i < 300; i++) {
    bits.push({ i: i, s: "x" });
  }
  this.n = n;
  this.len = bits.length;
}

function NewTargetMarker(this: any): void {}

function makeClassExpression(): any {
  return class {
    n: number;
    len: number;
    constructor(n: number) {
      const bits: any[] = [];
      for (let i = 0; i < 300; i++) {
        bits.push({ i: i, s: "x" });
      }
      this.n = n;
      this.len = bits.length;
    }
  };
}

// The receiver is read out of a property, so the callee is a runtime value and
// codegen has to route the construction through the runtime helper.
function constructDynamic(holder: any, n: number): number {
  let bad = 0;
  const inst = new holder.ctor(n);
  if (inst === null || inst === undefined) {
    return 1;
  }
  if ((inst.n as number) !== n) {
    bad++;
  }
  if ((inst.len as number) !== 300) {
    bad++;
  }
  return bad;
}

function runDeclaredClass(): number {
  let bad = 0;
  for (let r = 0; r < 200; r++) {
    bad += constructDynamic({ ctor: DeclaredPayload }, r);
  }
  return bad;
}

function runPlainFunction(): number {
  let bad = 0;
  for (let r = 0; r < 200; r++) {
    bad += constructDynamic({ ctor: FunctionPayload }, r);
  }
  return bad;
}

function runClassExpression(): number {
  const Made = makeClassExpression();
  let bad = 0;
  for (let r = 0; r < 200; r++) {
    bad += constructDynamic({ ctor: Made }, r);
  }
  return bad;
}

function runReflectConstruct(): number {
  let bad = 0;
  for (let r = 0; r < 200; r++) {
    const inst: any = Reflect.construct(FunctionPayload, [r], NewTargetMarker);
    if (inst === null || inst === undefined) {
      bad++;
    } else {
      if ((inst.n as number) !== r) {
        bad++;
      }
      if ((inst.len as number) !== 300) {
        bad++;
      }
    }
  }
  return bad;
}

console.log("declared", runDeclaredClass());
console.log("function", runPlainFunction());
console.log("classexpr", runClassExpression());
console.log("reflect", runReflectConstruct());
