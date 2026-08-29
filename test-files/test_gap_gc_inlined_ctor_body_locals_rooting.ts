// parity-env: PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_SCHEDULE_SEED=9081 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1 PERRY_GC_PROTECT_FROMSPACE=1
//
// #9081: pointer locals of a constructor body INLINED into another function's
// frame must get shadow slots in THAT frame.
//
// Codegen splices constructor bodies inline at four kinds of sites: the
// `super(...)` parent-body inline inside a derived constructor
// (`expr/this_super_call.rs`), the `new`-site own-ctor and inherited-ctor
// inlines (`lower_call/new.rs`), and `let_stmt`'s scalar-ctor variants. Each
// enclosing function's shadow-slot map is computed by
// `collect_pointer_typed_locals` over that function's OWN params and body, so
// a spliced body's `Let`s (and its bound ctor params) landed in plain entry
// allocas — neither shadow slots nor temp roots — invisible to the collector.
// A moving minor between the local's store and a later use then leaves it
// holding the pre-move address.
//
// This is how three.js `new WebGLRenderTarget()` died under a small nursery:
// `RenderTarget`'s ctor body — spliced by the super() inline into the
// standalone `WebGLRenderTarget_constructor`, which the entry module CALLS as
// a symbol — holds `const texture = new Texture(image)` across
// `this.textures = []` and the attachment loop; the retired-from-space
// `texture` then made `Texture.copy()` read `source.mipmaps` as undefined
// ("Cannot read properties of undefined (reading 'slice')").
//
// The dynamic arm below reproduces that exact configuration: constructing
// through `holder.ctor` routes through `js_new_function_construct`, which
// replays the STANDALONE derived constructor — the compiled method whose
// frame contains the super() splice of Base's body. The direct-new arms cover
// the `new`-site splices.
//
// OBSERVABLE AFTER THE SPLICED LOCAL GOES STALE, with no dependence on lucky
// from-space reuse: each round also stores the vulnerable local's object into
// an instance field, so the object is reachable and MOVES on every minor (the
// collector rewrites the instance field but cannot see the spliced local).
// After the churn, `stampRound` writes the round number through the rewritten
// path (an opaque function boundary), and the round reads the stamp back
// THROUGH THE SPLICED LOCAL. A stale local still points at the first
// pre-churn copy, which never received the write — and under the seeded
// every-poll schedule that copy's pages have long been retired and recycled,
// so the read comes back as garbage instead of the round number.
//
// THE `parity-env` LINE IS PART OF THE TEST: RATE=1 + ALLOC_KB=0 turns every
// loop back-edge poll into an evacuating minor, so the churn loop guarantees
// collections inside the vulnerable window deterministically instead of
// relying on allocation pacing.

class Payload {
  arr: number[];
  stamp: number;
  constructor() {
    this.arr = [1, 2, 3];
    this.stamp = -1;
  }
}

// Opaque write path for the test: writes land through the collector-rewritten
// instance fields, never through the spliced body's locals.
function stampRound(o: Base, n: number): void {
  o.p.stamp = n;
  o.opts.stamp = n;
}

class Base {
  p: Payload;
  opts: any;
  fromLocal: number;
  fromParam: number;
  kept: number;
  constructor(n: number, opts: any) {
    // The vulnerable spliced LOCAL, and the vulnerable spliced PARAM
    // (`opts`): both live in plain allocas of the enclosing frame when this
    // body is inlined. Rooting them through `this` gives the collector a
    // path it DOES rewrite, so a move makes the two paths diverge.
    const payload = new Payload();
    this.p = payload;
    this.opts = opts;
    // Every back-edge of this loop is an evacuating minor under the seeded
    // schedule; the allocations keep the heap live enough that from-space
    // pages are recycled before the reads below.
    let window: any[] = [];
    let kept = 0;
    for (let i = 0; i < 64; i++) {
      window.push({ i: i, s: "pad-" + i });
      if (window.length === 16) {
        kept += window.length;
        window = [];
      }
    }
    stampRound(this, n);
    // Read back through the spliced local/param. Stale copies never saw
    // `stampRound`'s writes.
    this.fromLocal = payload.stamp;
    this.fromParam = opts.stamp;
    this.kept = kept;
  }
}

// Own ctor calling super(): Base's body is spliced into this constructor's
// frame by the super() parent-body inline.
class DerivedOwnCtor extends Base {
  m: number;
  constructor(n: number, opts: any) {
    super(n, opts);
    this.m = n * 2;
  }
}

// No own ctor: Base's body is spliced into the frame of whatever function
// lowers a direct `new DerivedDefaultCtor(...)` site (inherited-ctor inline).
class DerivedDefaultCtor extends Base {}

function checkOne(o: Base, n: number): number {
  let bad = 0;
  if (o.fromLocal !== n) {
    bad++;
  }
  if (o.fromParam !== n) {
    bad++;
  }
  if (o.p.arr.length !== 3 || o.p.arr[0] !== 1 || o.p.arr[2] !== 3) {
    bad++;
  }
  if (o.kept !== 64) {
    bad++;
  }
  return bad;
}

// The three.js configuration: a runtime-dispatched construction replays the
// STANDALONE derived constructor, whose own frame holds the super() splice.
function constructDynamic(holder: any, n: number): Base {
  return new holder.ctor(n, { stamp: -1 });
}

function runDynamicOwnCtor(): number {
  let bad = 0;
  const holder = { ctor: DerivedOwnCtor };
  for (let r = 0; r < 6; r++) {
    bad += checkOne(constructDynamic(holder, r), r);
  }
  return bad;
}

function runOwnCtor(): number {
  let bad = 0;
  for (let r = 0; r < 6; r++) {
    bad += checkOne(new DerivedOwnCtor(r, { stamp: -1 }), r);
  }
  return bad;
}

function runDefaultCtor(): number {
  let bad = 0;
  for (let r = 0; r < 6; r++) {
    bad += checkOne(new DerivedDefaultCtor(r, { stamp: -1 }), r);
  }
  return bad;
}

function runBaseDirect(): number {
  // The `new`-site own-ctor inline: Base's body spliced into this function.
  let bad = 0;
  for (let r = 0; r < 6; r++) {
    bad += checkOne(new Base(r, { stamp: -1 }), r);
  }
  return bad;
}

console.log("dynamic", runDynamicOwnCtor());
console.log("own", runOwnCtor());
console.log("default", runDefaultCtor());
console.log("base", runBaseDirect());
