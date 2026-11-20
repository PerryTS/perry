// #9422: a rejected ordinary-object [[Set]] throws ONLY in strict mode -- and
// perry never threw at all.
//
// ES2024 SS6.2.5.7 (PutValue) calls `Set(O, P, V, Throw)` with
// Throw = IsStrictReference(ref), and SS10.1.9 (OrdinarySet) reports `false`
// for a non-writable data property, an accessor with no setter, and a new
// property on a non-extensible object. So each of those is a silent no-op in
// sloppy code and a TypeError in strict code. `Object.freeze` is the shape
// real programs rely on: a silent no-op there means a program that should have
// crashed keeps running on stale state.
//
// This is the mirror image of #9394 (fixed in #9426), which was the ARRAY
// element path throwing in sloppy mode where node is silent. That fixture's
// ordinary-object control appears in its sloppy arm only, with a comment
// pointing here -- because the strict object arm was silent too.
//
// This file is `.cts`, so it is a CommonJS script in BOTH runtimes: `sloppyArm`
// is sloppy code and `strictArm` opts in with its own directive prologue.
// BOTH ARMS ARE ASSERTED. Asserting only the throw is what let #9394 through,
// and asserting only the sloppy no-op is what let THIS through.
//
// The two arms are textual duplicates on purpose: a function inherits the
// strictness of the code it is DEFINED in, never its caller's, so a shared
// helper would test sloppy twice. Only the mode prefix and the directive
// differ.
//
// The module (ESM, always-strict) half of the same shapes lives in
// test_gap_9423_module_init_strictness.ts.

function report(name: string, threw: boolean, ...rest: unknown[]): void {
  console.log(name, threw ? "TypeError" : "silent", ...rest);
}

function hasOwn(value: any, key: PropertyKey): boolean {
  return Object.prototype.hasOwnProperty.call(value, key);
}

function nonWritableProto(): any {
  const proto: any = {};
  Object.defineProperty(proto, "x", {
    configurable: true,
    enumerable: true,
    value: "protoX",
    writable: false,
  });
  return proto;
}

function getterOnlyProto(): any {
  const proto: any = {};
  Object.defineProperty(proto, "x", {
    configurable: true,
    get() {
      return "getterOnlyX";
    },
  });
  return proto;
}

function setterProto(calls: any[]): any {
  const proto: any = {};
  Object.defineProperty(proto, "x", {
    configurable: true,
    get() {
      return "accX";
    },
    set(value: any) {
      calls.push(value);
    },
  });
  return proto;
}

class Cell {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
}

function sloppyArm(): void {
  let threw = false;

  // 1. Frozen object, existing own property.
  const frozen: any = { x: 1 };
  Object.freeze(frozen);
  threw = false;
  try {
    frozen.x = 9;
  } catch {
    threw = true;
  }
  report("sloppy frozen own:", threw, frozen.x);

  // 2. Frozen object, NEW property (non-extensible half of freeze).
  const frozenNew: any = { x: 1 };
  Object.freeze(frozenNew);
  threw = false;
  try {
    frozenNew.y = 9;
  } catch {
    threw = true;
  }
  report("sloppy frozen new:", threw, hasOwn(frozenNew, "y"));

  // 3. Sealed object, existing own property -- seal leaves it WRITABLE, so
  //    this succeeds in both modes. The over-throw control.
  const sealed: any = { x: 1 };
  Object.seal(sealed);
  threw = false;
  try {
    sealed.x = 9;
  } catch {
    threw = true;
  }
  report("sloppy sealed own:", threw, sealed.x);

  // 4. Sealed object, NEW property.
  const sealedNew: any = { x: 1 };
  Object.seal(sealedNew);
  threw = false;
  try {
    sealedNew.y = 9;
  } catch {
    threw = true;
  }
  report("sloppy sealed new:", threw, hasOwn(sealedNew, "y"));

  // 5. Non-writable OWN data property.
  const readOnly: any = {};
  Object.defineProperty(readOnly, "x", {
    configurable: true,
    enumerable: true,
    value: 1,
    writable: false,
  });
  threw = false;
  try {
    readOnly.x = 9;
  } catch {
    threw = true;
  }
  report("sloppy non-writable own:", threw, readOnly.x);

  // 6. Non-writable INHERITED data property: OrdinarySetWithOwnDescriptor
  //    consults the prototype chain BEFORE creating an own property, so this
  //    is rejected and no own property appears.
  const inheritedReadOnly: any = Object.create(nonWritableProto());
  threw = false;
  try {
    inheritedReadOnly.x = 9;
  } catch {
    threw = true;
  }
  report(
    "sloppy non-writable inherited:",
    threw,
    hasOwn(inheritedReadOnly, "x"),
    inheritedReadOnly.x,
  );

  // 7. Getter-only OWN accessor.
  const getterOnly: any = {};
  Object.defineProperty(getterOnly, "x", {
    configurable: true,
    get() {
      return "ownGetter";
    },
  });
  threw = false;
  try {
    getterOnly.x = 9;
  } catch {
    threw = true;
  }
  report("sloppy getter-only own:", threw, getterOnly.x);

  // 8. Getter-only INHERITED accessor.
  const inheritedGetterOnly: any = Object.create(getterOnlyProto());
  threw = false;
  try {
    inheritedGetterOnly.x = 9;
  } catch {
    threw = true;
  }
  report(
    "sloppy getter-only inherited:",
    threw,
    hasOwn(inheritedGetterOnly, "x"),
    inheritedGetterOnly.x,
  );

  // 9. An inherited SETTER runs in both modes and creates no own property.
  const calls: any[] = [];
  const withSetter: any = Object.create(setterProto(calls));
  threw = false;
  try {
    withSetter.x = 11;
  } catch {
    threw = true;
  }
  report("sloppy inherited setter:", threw, calls.join(","), hasOwn(withSetter, "x"));

  // 10. preventExtensions, NEW property.
  const noExtend: any = { x: 1 };
  Object.preventExtensions(noExtend);
  threw = false;
  try {
    noExtend.y = 9;
  } catch {
    threw = true;
  }
  report("sloppy preventExtensions new:", threw, hasOwn(noExtend, "y"));

  // 11. preventExtensions, EXISTING property -- succeeds in both modes.
  const noExtendOwn: any = { x: 1 };
  Object.preventExtensions(noExtendOwn);
  threw = false;
  try {
    noExtendOwn.x = 9;
  } catch {
    threw = true;
  }
  report("sloppy preventExtensions own:", threw, noExtendOwn.x);

  // 12. Computed (dynamic) key on a frozen object -- a different store lane
  //     from the static-name one above.
  const frozenComputed: any = { x: 1 };
  Object.freeze(frozenComputed);
  const key = "x";
  threw = false;
  try {
    frozenComputed[key] = 9;
  } catch {
    threw = true;
  }
  report("sloppy frozen computed:", threw, frozenComputed.x);

  // 13. Class-field store on a frozen instance -- the class-field store lane.
  const cell = new Cell(1);
  Object.freeze(cell);
  threw = false;
  try {
    cell.v = 9;
  } catch {
    threw = true;
  }
  report("sloppy frozen class field:", threw, cell.v);

  // 14. Update (`++`) on a frozen object: `PutValue` runs with the same Throw
  //     flag after the read, so a rejected update is silent here.
  //
  //     `o.x += 1` is DELIBERATELY ABSENT from this arm. Perry throws for it
  //     in sloppy code, where node is silent -- an OVER-throw on a different
  //     lane: `+=` lowers to `Expr::PropertySet`, an HIR node that carries no
  //     strictness at all, and its codegen reaches
  //     `js_typed_feedback_object_set_field_by_name`, which has no `strict`
  //     parameter and rejects by throwing. (`++` lowers to
  //     `Expr::PropertyUpdate`, which DOES carry `ctx.current_strict`, and is
  //     correct -- hence both are named here.) That is #9394's shape on the
  //     object path, the opposite direction from what this file is about, and
  //     it is not fixed here. The strict arm keeps its `+=` case, where the
  //     unconditional throw happens to be the right answer.
  const frozenUpdate: any = { x: 1 };
  Object.freeze(frozenUpdate);
  threw = false;
  try {
    frozenUpdate.x++;
  } catch {
    threw = true;
  }
  report("sloppy frozen update:", threw, frozenUpdate.x);

  // 15. Array `length` and element stores on a frozen array, on the OBJECT
  //     (named-property) lane -- `a.length = n` is `Set(O,"length",n,Throw)`.
  const frozenArray: any[] = [1, 2];
  Object.freeze(frozenArray);
  threw = false;
  try {
    frozenArray.length = 0;
  } catch {
    threw = true;
  }
  report("sloppy frozen array length:", threw, frozenArray.length);

  const nonWritableLength: any[] = [1, 2];
  Object.defineProperty(nonWritableLength, "length", { writable: false });
  threw = false;
  try {
    nonWritableLength.length = 0;
  } catch {
    threw = true;
  }
  report("sloppy non-writable array length:", threw, nonWritableLength.length);

  const frozenArrayIndex: any[] = [1, 2];
  Object.freeze(frozenArrayIndex);
  threw = false;
  try {
    frozenArrayIndex[0] = 9;
  } catch {
    threw = true;
  }
  report("sloppy frozen array index:", threw, frozenArrayIndex[0]);
}

function strictArm(): void {
  "use strict";

  let threw = false;

  // 1. Frozen object, existing own property.
  const frozen: any = { x: 1 };
  Object.freeze(frozen);
  threw = false;
  try {
    frozen.x = 9;
  } catch {
    threw = true;
  }
  report("strict frozen own:", threw, frozen.x);

  // 2. Frozen object, NEW property (non-extensible half of freeze).
  const frozenNew: any = { x: 1 };
  Object.freeze(frozenNew);
  threw = false;
  try {
    frozenNew.y = 9;
  } catch {
    threw = true;
  }
  report("strict frozen new:", threw, hasOwn(frozenNew, "y"));

  // 3. Sealed object, existing own property -- seal leaves it WRITABLE, so
  //    this succeeds in both modes. The over-throw control.
  const sealed: any = { x: 1 };
  Object.seal(sealed);
  threw = false;
  try {
    sealed.x = 9;
  } catch {
    threw = true;
  }
  report("strict sealed own:", threw, sealed.x);

  // 4. Sealed object, NEW property.
  const sealedNew: any = { x: 1 };
  Object.seal(sealedNew);
  threw = false;
  try {
    sealedNew.y = 9;
  } catch {
    threw = true;
  }
  report("strict sealed new:", threw, hasOwn(sealedNew, "y"));

  // 5. Non-writable OWN data property.
  const readOnly: any = {};
  Object.defineProperty(readOnly, "x", {
    configurable: true,
    enumerable: true,
    value: 1,
    writable: false,
  });
  threw = false;
  try {
    readOnly.x = 9;
  } catch {
    threw = true;
  }
  report("strict non-writable own:", threw, readOnly.x);

  // 6. Non-writable INHERITED data property.
  const inheritedReadOnly: any = Object.create(nonWritableProto());
  threw = false;
  try {
    inheritedReadOnly.x = 9;
  } catch {
    threw = true;
  }
  report(
    "strict non-writable inherited:",
    threw,
    hasOwn(inheritedReadOnly, "x"),
    inheritedReadOnly.x,
  );

  // 7. Getter-only OWN accessor.
  const getterOnly: any = {};
  Object.defineProperty(getterOnly, "x", {
    configurable: true,
    get() {
      return "ownGetter";
    },
  });
  threw = false;
  try {
    getterOnly.x = 9;
  } catch {
    threw = true;
  }
  report("strict getter-only own:", threw, getterOnly.x);

  // 8. Getter-only INHERITED accessor.
  const inheritedGetterOnly: any = Object.create(getterOnlyProto());
  threw = false;
  try {
    inheritedGetterOnly.x = 9;
  } catch {
    threw = true;
  }
  report(
    "strict getter-only inherited:",
    threw,
    hasOwn(inheritedGetterOnly, "x"),
    inheritedGetterOnly.x,
  );

  // 9. An inherited SETTER runs in both modes and creates no own property.
  const calls: any[] = [];
  const withSetter: any = Object.create(setterProto(calls));
  threw = false;
  try {
    withSetter.x = 11;
  } catch {
    threw = true;
  }
  report("strict inherited setter:", threw, calls.join(","), hasOwn(withSetter, "x"));

  // 10. preventExtensions, NEW property.
  const noExtend: any = { x: 1 };
  Object.preventExtensions(noExtend);
  threw = false;
  try {
    noExtend.y = 9;
  } catch {
    threw = true;
  }
  report("strict preventExtensions new:", threw, hasOwn(noExtend, "y"));

  // 11. preventExtensions, EXISTING property -- succeeds in both modes.
  const noExtendOwn: any = { x: 1 };
  Object.preventExtensions(noExtendOwn);
  threw = false;
  try {
    noExtendOwn.x = 9;
  } catch {
    threw = true;
  }
  report("strict preventExtensions own:", threw, noExtendOwn.x);

  // 12. Computed (dynamic) key on a frozen object.
  const frozenComputed: any = { x: 1 };
  Object.freeze(frozenComputed);
  const key = "x";
  threw = false;
  try {
    frozenComputed[key] = 9;
  } catch {
    threw = true;
  }
  report("strict frozen computed:", threw, frozenComputed.x);

  // 13. Class-field store on a frozen instance.
  const cell = new Cell(1);
  Object.freeze(cell);
  threw = false;
  try {
    cell.v = 9;
  } catch {
    threw = true;
  }
  report("strict frozen class field:", threw, cell.v);

  // 14. Compound assignment and update on a frozen object. The sloppy arm has
  //     no `+=` twin -- see the note there.
  const frozenCompound: any = { x: 1 };
  Object.freeze(frozenCompound);
  threw = false;
  try {
    frozenCompound.x += 1;
  } catch {
    threw = true;
  }
  report("strict frozen compound:", threw, frozenCompound.x);

  const frozenUpdate: any = { x: 1 };
  Object.freeze(frozenUpdate);
  threw = false;
  try {
    frozenUpdate.x++;
  } catch {
    threw = true;
  }
  report("strict frozen update:", threw, frozenUpdate.x);

  // 15. Array `length` and element stores on a frozen array.
  const frozenArray: any[] = [1, 2];
  Object.freeze(frozenArray);
  threw = false;
  try {
    frozenArray.length = 0;
  } catch {
    threw = true;
  }
  report("strict frozen array length:", threw, frozenArray.length);

  const nonWritableLength: any[] = [1, 2];
  Object.defineProperty(nonWritableLength, "length", { writable: false });
  threw = false;
  try {
    nonWritableLength.length = 0;
  } catch {
    threw = true;
  }
  report("strict non-writable array length:", threw, nonWritableLength.length);

  const frozenArrayIndex: any[] = [1, 2];
  Object.freeze(frozenArrayIndex);
  threw = false;
  try {
    frozenArrayIndex[0] = 9;
  } catch {
    threw = true;
  }
  report("strict frozen array index:", threw, frozenArrayIndex[0]);
}

sloppyArm();
strictArm();
