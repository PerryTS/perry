// #7143 repro fixture: module A. Declares class C (three declared fields)
// and a method that reads a declared field through `this` — eligible for the
// Phase 5a proven-`this` `$pshape` clone as long as THIS module contains no
// `delete`/`Reflect.deleteProperty`/other §5.2 shape-barrier expression.
//
// `readViaA` is the call site that matters: `inst` is a plain typed
// parameter (NOT a locally-`new`'d Ptr<Shape> local), so the receiver is
// aliased by construction — exactly the #7143 scenario — and the call
// routes through the GUARDED `method_direct.fast` site
// (`lower_call/method_override.rs`), not the containment-proof Phase 3b
// `Ptr<Shape>` receiver arm.
export class C {
  a: number = 1;
  b: number = 2;
  c: number = 3;

  readC(): number {
    return this.c;
  }
}

export function makeC(): C {
  return new C();
}

export function readViaA(inst: C): number {
  return inst.readC();
}
