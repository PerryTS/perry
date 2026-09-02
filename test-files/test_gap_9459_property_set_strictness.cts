// #9459: `Expr::PropertySet` carries no strictness field, so every assignment
// form that lowers to it threw on a rejected `[[Set]]` in SLOPPY code, where
// node is silent.
//
// ES2024 SS6.2.5.7 (PutValue) performs `Set(O, P, V, Throw)` with
// `Throw = IsStrictReference(ref)`, and SS10.1.9 (OrdinarySet) reports `false`
// -- not a throw -- for a non-writable own or inherited data property, an
// accessor with no setter, and a new property on a non-extensible object. The
// reference's strictness is what turns that `false` into a TypeError.
//
// Three spellings of the same store disagreed on main:
//
//   o.x = 9      -> `Expr::PutValueSet`  (carries `strict`)         CORRECT
//   o.x++        -> `Expr::PropertyUpdate` (carries `strict`)       CORRECT
//   o.x += 1     -> `Expr::PropertySet`  (carries NOTHING)          THREW
//   for (o.x of) -> `Expr::PropertySet`                             THREW
//   [o.x] = arr  -> `Expr::PropertySet` (expression position)       THREW
//
// This is the object-path mirror of #9394 (arrays, fixed by #9426) and the
// opposite direction from #9422 (which was an UNDER-throw in strict code).
//
// This file is `.cts`, so it is a CommonJS script in BOTH runtimes: `sloppyArm`
// is sloppy code and `strictArm` opts in with its own directive prologue.
// BOTH ARMS ARE ASSERTED. Asserting only the sloppy no-op is what let #9422
// through, and asserting only the strict throw is what let this through.
//
// The two arms are textual duplicates on purpose: a function inherits the
// strictness of the code it is DEFINED in, never its caller's, so a shared
// helper would test one mode twice. Only the mode prefix and the directive
// differ.
//
// Companions: test_gap_9422_strict_object_store_strictness.cts (the `=` lane),
// test_gap_9394_array_element_store_strictness.cts (the array element lane),
// test_gap_9423_module_init_strictness.ts (the ESM always-strict half).

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
    value: 10,
    writable: false,
  });
  return proto;
}

function getterOnlyProto(): any {
  const proto: any = {};
  Object.defineProperty(proto, "x", {
    configurable: true,
    get() {
      return 20;
    },
  });
  return proto;
}

function setterProto(calls: any[]): any {
  const proto: any = {};
  Object.defineProperty(proto, "x", {
    configurable: true,
    get() {
      return 30;
    },
    set(value: any) {
      calls.push(value);
    },
  });
  return proto;
}

function readOnlyOwn(initial: any): any {
  const o: any = {};
  Object.defineProperty(o, "x", {
    configurable: true,
    enumerable: true,
    value: initial,
    writable: false,
  });
  return o;
}

function getterOnlyOwn(): any {
  const o: any = {};
  Object.defineProperty(o, "x", {
    configurable: true,
    enumerable: true,
    get() {
      return 40;
    },
  });
  return o;
}

class Cell {
  x: number;
  constructor(x: number) {
    this.x = x;
  }
}

function frozenCell(): any {
  const c = new Cell(1);
  Object.freeze(c);
  return c;
}

function sloppyArm(): void {
  let threw = false;

  // ---- compound assignment, every operator, on a frozen own data property ----
  const plus: any = { x: 1 };
  Object.freeze(plus);
  threw = false;
  try {
    plus.x += 1;
  } catch {
    threw = true;
  }
  report("sloppy frozen +=:", threw, plus.x);

  const minus: any = { x: 1 };
  Object.freeze(minus);
  threw = false;
  try {
    minus.x -= 1;
  } catch {
    threw = true;
  }
  report("sloppy frozen -=:", threw, minus.x);

  const times: any = { x: 3 };
  Object.freeze(times);
  threw = false;
  try {
    times.x *= 2;
  } catch {
    threw = true;
  }
  report("sloppy frozen *=:", threw, times.x);

  // Logical assignment writes only on the branch that reaches the store, so
  // each operator gets a starting value that makes it write.
  const andAnd: any = { x: 1 };
  Object.freeze(andAnd);
  threw = false;
  try {
    andAnd.x &&= 5;
  } catch {
    threw = true;
  }
  report("sloppy frozen &&=:", threw, andAnd.x);

  const orOr: any = { x: 0 };
  Object.freeze(orOr);
  threw = false;
  try {
    orOr.x ||= 5;
  } catch {
    threw = true;
  }
  report("sloppy frozen ||=:", threw, orOr.x);

  const nullish: any = { x: undefined };
  Object.freeze(nullish);
  threw = false;
  try {
    nullish.x ??= 5;
  } catch {
    threw = true;
  }
  report("sloppy frozen ??=:", threw, nullish.x);

  // The short-circuit CONTROL: `&&=` on a falsy value never stores, so it is
  // silent in both modes even on a frozen receiver. A fix that made every
  // logical assignment throw would show up here.
  const andShort: any = { x: 0 };
  Object.freeze(andShort);
  threw = false;
  try {
    andShort.x &&= 5;
  } catch {
    threw = true;
  }
  report("sloppy frozen &&= short-circuit:", threw, andShort.x);

  // ---- for-of head, named and computed ----
  const forNamed: any = { x: 1 };
  Object.freeze(forNamed);
  threw = false;
  try {
    for (forNamed.x of [7]) {
    }
  } catch {
    threw = true;
  }
  report("sloppy frozen for-of head:", threw, forNamed.x);

  const forComputed: any = { x: 1 };
  Object.freeze(forComputed);
  const forKey = "x";
  threw = false;
  try {
    for (forComputed[forKey] of [7]) {
    }
  } catch {
    threw = true;
  }
  report("sloppy frozen for-of head computed:", threw, forComputed.x);

  // The computed-key twin of `+=` above. `o[k] += 1` and `for (o[k] of ...)`
  // both lower to `Expr::IndexSet`, whose OBJECT-by-name lanes kept throwing
  // after #9426 carried the flag to its array element lanes.
  const computedPlus: any = { x: 1 };
  Object.freeze(computedPlus);
  const computedKey = "x";
  threw = false;
  try {
    computedPlus[computedKey] += 1;
  } catch {
    threw = true;
  }
  report("sloppy frozen o[k] +=:", threw, computedPlus.x);

  // A LITERAL computed key takes a different `Expr::IndexSet` arm from a
  // runtime string key, so both are asserted.
  const literalPlus: any = { x: 1 };
  Object.freeze(literalPlus);
  threw = false;
  try {
    literalPlus["x"] += 1;
  } catch {
    threw = true;
  }
  report("sloppy frozen o[\"x\"] +=:", threw, literalPlus.x);

  // And an UNTYPED key, which reaches the runtime STRING_TAG dispatch rather
  // than either static arm.
  const anyKeyed: any = { x: 1 };
  Object.freeze(anyKeyed);
  const anyKey: any = "x";
  threw = false;
  try {
    anyKeyed[anyKey] += 1;
  } catch {
    threw = true;
  }
  report("sloppy frozen o[anyKey] +=:", threw, anyKeyed.x);

  // ---- destructuring assignment targets ----
  // Statement position and expression position are DIFFERENT lowerings
  // (`destructuring/assignment_stmt.rs` vs `destructuring/assignment_expr.rs`),
  // so both are asserted.
  const arrDestr: any = { x: 1 };
  Object.freeze(arrDestr);
  threw = false;
  try {
    [arrDestr.x] = [7];
  } catch {
    threw = true;
  }
  report("sloppy frozen [o.x] = arr:", threw, arrDestr.x);

  const arrDestrExpr: any = { x: 1 };
  Object.freeze(arrDestrExpr);
  threw = false;
  try {
    const seen = ([arrDestrExpr.x] = [7]);
    void seen;
  } catch {
    threw = true;
  }
  report("sloppy frozen ([o.x] = arr) expr:", threw, arrDestrExpr.x);

  const objDestr: any = { x: 1 };
  Object.freeze(objDestr);
  threw = false;
  try {
    ({ a: objDestr.x } = { a: 7 });
  } catch {
    threw = true;
  }
  report("sloppy frozen ({a: o.x}) = obj:", threw, objDestr.x);

  const arrDestrComputed: any = { x: 1 };
  Object.freeze(arrDestrComputed);
  const destrKey = "x";
  threw = false;
  try {
    [arrDestrComputed[destrKey]] = [7];
  } catch {
    threw = true;
  }
  report("sloppy frozen [o[k]] = arr:", threw, arrDestrComputed.x);

  // ---- the same `+=` against every rejecting receiver shape ----
  const sealedOwn: any = { x: 1 };
  Object.seal(sealedOwn);
  threw = false;
  try {
    sealedOwn.x += 1;
  } catch {
    threw = true;
  }
  report("sloppy sealed own +=:", threw, sealedOwn.x);

  // `seal` leaves existing properties WRITABLE, so the line above must succeed
  // in both modes -- it is the over-throw control. A NEW key on a sealed object
  // is the rejecting half.
  const sealedNew: any = { x: 1 };
  Object.seal(sealedNew);
  threw = false;
  try {
    sealedNew.y += 1;
  } catch {
    threw = true;
  }
  report("sloppy sealed new +=:", threw, hasOwn(sealedNew, "y"));

  const nonWritable = readOnlyOwn(1);
  threw = false;
  try {
    nonWritable.x += 1;
  } catch {
    threw = true;
  }
  report("sloppy non-writable own +=:", threw, nonWritable.x);

  // ---- INHERITED rejecting receivers ----
  //
  // `+=` against a receiver whose rejection lives on the PROTOTYPE is asserted
  // in the sloppy arm only, and the strict twin is spelled `=` instead. That
  // asymmetry is deliberate and is NOT what this issue is about:
  //
  //   Perry's `Expr::PropertySet` tail (`js_object_set_field_by_name`) performs
  //   an OWN-property store. It never runs `OrdinarySetWithOwnDescriptor`'s
  //   prototype walk, so an inherited non-writable data property, an inherited
  //   getter-only accessor, and an inherited SETTER are all mishandled the same
  //   way: the setter never fires and an own property is created instead. That
  //   is wrong in BOTH modes, it is wrong on unfixed `main` in both modes, and
  //   it is a missing prototype walk rather than a missing `Throw` flag.
  //
  //   #9459 routes the SLOPPY tail to `js_put_value_set(..., 0)` -- the
  //   receiver-aware `[[Set]]` that sloppy `o.x = v` has always used -- so the
  //   sloppy arm below becomes correct as a side effect of getting the
  //   strictness right. The strict tail keeps its typed-feedback store site
  //   (`js_typed_feedback_object_set_field_by_name_fast`, a #7480/#5093 gate
  //   with its own IR tests), so fixing the strict half means retargeting that
  //   lane, which is a separate change. Filed as #9495; the strict `=`
  //   twins below pin the shapes so a future fix has a baseline here.
  //
  // Asserting the sloppy arm alone would be the #9394 mistake, which is why the
  // strict side is still exercised -- on the lane that is already correct.
  const inheritedNonWritable: any = Object.create(nonWritableProto());
  threw = false;
  try {
    inheritedNonWritable.x += 1;
  } catch {
    threw = true;
  }
  report(
    "sloppy non-writable inherited +=:",
    threw,
    hasOwn(inheritedNonWritable, "x"),
    inheritedNonWritable.x,
  );

  const getterOnly = getterOnlyOwn();
  threw = false;
  try {
    getterOnly.x += 1;
  } catch {
    threw = true;
  }
  report("sloppy getter-only own +=:", threw, getterOnly.x);

  const inheritedGetterOnly: any = Object.create(getterOnlyProto());
  threw = false;
  try {
    inheritedGetterOnly.x += 1;
  } catch {
    threw = true;
  }
  report(
    "sloppy getter-only inherited +=:",
    threw,
    hasOwn(inheritedGetterOnly, "x"),
    inheritedGetterOnly.x,
  );

  // An inherited SETTER runs in both modes and creates no own property: the
  // rejection is about `[[Set]]` returning false, never about reaching the
  // accessor. A fix that routed sloppy stores past the prototype walk would
  // show up here as a missing call.
  const calls: any[] = [];
  const withSetter: any = Object.create(setterProto(calls));
  threw = false;
  try {
    withSetter.x += 1;
  } catch {
    threw = true;
  }
  report("sloppy inherited setter +=:", threw, calls.join(","), hasOwn(withSetter, "x"));

  const noExtend: any = { x: 1 };
  Object.preventExtensions(noExtend);
  threw = false;
  try {
    noExtend.y += 1;
  } catch {
    threw = true;
  }
  report("sloppy preventExtensions new +=:", threw, hasOwn(noExtend, "y"));

  // preventExtensions leaves existing properties writable -- the second
  // over-throw control.
  const noExtendOwn: any = { x: 1 };
  Object.preventExtensions(noExtendOwn);
  threw = false;
  try {
    noExtendOwn.x += 1;
  } catch {
    threw = true;
  }
  report("sloppy preventExtensions own +=:", threw, noExtendOwn.x);

  // ---- the class-field store lane ----
  const cell = frozenCell();
  threw = false;
  try {
    cell.x += 1;
  } catch {
    threw = true;
  }
  report("sloppy frozen class field +=:", threw, cell.x);

  const liveCell = new Cell(1);
  threw = false;
  try {
    liveCell.x += 1;
  } catch {
    threw = true;
  }
  report("sloppy live class field +=:", threw, liveCell.x);

  // ---- the `arr.length` lane ----
  // `a.length += n` is `Set(O, "length", n, Throw)` on the OBJECT lane, which
  // reaches `js_array_set_length_strict` -- named for the `Throw` it hard-codes.
  const frozenArray: any[] = [1, 2];
  Object.freeze(frozenArray);
  threw = false;
  try {
    frozenArray.length += 1;
  } catch {
    threw = true;
  }
  report("sloppy frozen array length +=:", threw, frozenArray.length);

  const nonWritableLength: any[] = [1, 2];
  Object.defineProperty(nonWritableLength, "length", { writable: false });
  threw = false;
  try {
    nonWritableLength.length += 1;
  } catch {
    threw = true;
  }
  report("sloppy non-writable array length +=:", threw, nonWritableLength.length);

  // A live array's `length` accepts the write in both modes: the control that
  // proves the lane still WORKS, not just that it stopped throwing.
  const liveArray: any[] = [1, 2];
  threw = false;
  try {
    liveArray.length += 1;
  } catch {
    threw = true;
  }
  report("sloppy live array length +=:", threw, liveArray.length);

  // ---- accepted stores, to prove the sloppy tail still STORES ----
  const plain: any = { x: 1 };
  threw = false;
  try {
    plain.x += 41;
  } catch {
    threw = true;
  }
  report("sloppy plain +=:", threw, plain.x);

  const plainFor: any = { x: 1 };
  threw = false;
  try {
    for (plainFor.x of [7]) {
    }
  } catch {
    threw = true;
  }
  report("sloppy plain for-of head:", threw, plainFor.x);

  const plainDestr: any = { x: 1 };
  threw = false;
  try {
    [plainDestr.x] = [7];
  } catch {
    threw = true;
  }
  report("sloppy plain [o.x] = arr:", threw, plainDestr.x);

  const plainNew: any = { x: 1 };
  threw = false;
  try {
    plainNew.y ??= 9;
  } catch {
    threw = true;
  }
  report("sloppy plain new key ??=:", threw, plainNew.y);

  // ---- `++` (Expr::PropertyUpdate) alongside `+=`, so the two spellings of
  //      one operation are asserted in the same file and mode ----
  const upd: any = { x: 1 };
  Object.freeze(upd);
  threw = false;
  try {
    upd.x++;
  } catch {
    threw = true;
  }
  report("sloppy frozen ++:", threw, upd.x);

  // ---- and plain `=`, the lane that was already right (#9422) ----
  const assign: any = { x: 1 };
  Object.freeze(assign);
  threw = false;
  try {
    assign.x = 9;
  } catch {
    threw = true;
  }
  report("sloppy frozen =:", threw, assign.x);
}

function strictArm(): void {
  "use strict";

  let threw = false;

  // ---- compound assignment, every operator, on a frozen own data property ----
  const plus: any = { x: 1 };
  Object.freeze(plus);
  threw = false;
  try {
    plus.x += 1;
  } catch {
    threw = true;
  }
  report("strict frozen +=:", threw, plus.x);

  const minus: any = { x: 1 };
  Object.freeze(minus);
  threw = false;
  try {
    minus.x -= 1;
  } catch {
    threw = true;
  }
  report("strict frozen -=:", threw, minus.x);

  const times: any = { x: 3 };
  Object.freeze(times);
  threw = false;
  try {
    times.x *= 2;
  } catch {
    threw = true;
  }
  report("strict frozen *=:", threw, times.x);

  // Logical assignment writes only on the branch that reaches the store, so
  // each operator gets a starting value that makes it write.
  const andAnd: any = { x: 1 };
  Object.freeze(andAnd);
  threw = false;
  try {
    andAnd.x &&= 5;
  } catch {
    threw = true;
  }
  report("strict frozen &&=:", threw, andAnd.x);

  const orOr: any = { x: 0 };
  Object.freeze(orOr);
  threw = false;
  try {
    orOr.x ||= 5;
  } catch {
    threw = true;
  }
  report("strict frozen ||=:", threw, orOr.x);

  const nullish: any = { x: undefined };
  Object.freeze(nullish);
  threw = false;
  try {
    nullish.x ??= 5;
  } catch {
    threw = true;
  }
  report("strict frozen ??=:", threw, nullish.x);

  // The short-circuit CONTROL: `&&=` on a falsy value never stores, so it is
  // silent in both modes even on a frozen receiver. A fix that made every
  // logical assignment throw would show up here.
  const andShort: any = { x: 0 };
  Object.freeze(andShort);
  threw = false;
  try {
    andShort.x &&= 5;
  } catch {
    threw = true;
  }
  report("strict frozen &&= short-circuit:", threw, andShort.x);

  // ---- for-of head, named and computed ----
  const forNamed: any = { x: 1 };
  Object.freeze(forNamed);
  threw = false;
  try {
    for (forNamed.x of [7]) {
    }
  } catch {
    threw = true;
  }
  report("strict frozen for-of head:", threw, forNamed.x);

  const forComputed: any = { x: 1 };
  Object.freeze(forComputed);
  const forKey = "x";
  threw = false;
  try {
    for (forComputed[forKey] of [7]) {
    }
  } catch {
    threw = true;
  }
  report("strict frozen for-of head computed:", threw, forComputed.x);

  // The computed-key twin of `+=` above. `o[k] += 1` and `for (o[k] of ...)`
  // both lower to `Expr::IndexSet`, whose OBJECT-by-name lanes kept throwing
  // after #9426 carried the flag to its array element lanes.
  const computedPlus: any = { x: 1 };
  Object.freeze(computedPlus);
  const computedKey = "x";
  threw = false;
  try {
    computedPlus[computedKey] += 1;
  } catch {
    threw = true;
  }
  report("strict frozen o[k] +=:", threw, computedPlus.x);

  // A LITERAL computed key takes a different `Expr::IndexSet` arm from a
  // runtime string key, so both are asserted.
  const literalPlus: any = { x: 1 };
  Object.freeze(literalPlus);
  threw = false;
  try {
    literalPlus["x"] += 1;
  } catch {
    threw = true;
  }
  report("strict frozen o[\"x\"] +=:", threw, literalPlus.x);

  // And an UNTYPED key, which reaches the runtime STRING_TAG dispatch rather
  // than either static arm.
  const anyKeyed: any = { x: 1 };
  Object.freeze(anyKeyed);
  const anyKey: any = "x";
  threw = false;
  try {
    anyKeyed[anyKey] += 1;
  } catch {
    threw = true;
  }
  report("strict frozen o[anyKey] +=:", threw, anyKeyed.x);

  // ---- destructuring assignment targets ----
  // Statement position and expression position are DIFFERENT lowerings
  // (`destructuring/assignment_stmt.rs` vs `destructuring/assignment_expr.rs`),
  // so both are asserted.
  const arrDestr: any = { x: 1 };
  Object.freeze(arrDestr);
  threw = false;
  try {
    [arrDestr.x] = [7];
  } catch {
    threw = true;
  }
  report("strict frozen [o.x] = arr:", threw, arrDestr.x);

  const arrDestrExpr: any = { x: 1 };
  Object.freeze(arrDestrExpr);
  threw = false;
  try {
    const seen = ([arrDestrExpr.x] = [7]);
    void seen;
  } catch {
    threw = true;
  }
  report("strict frozen ([o.x] = arr) expr:", threw, arrDestrExpr.x);

  const objDestr: any = { x: 1 };
  Object.freeze(objDestr);
  threw = false;
  try {
    ({ a: objDestr.x } = { a: 7 });
  } catch {
    threw = true;
  }
  report("strict frozen ({a: o.x}) = obj:", threw, objDestr.x);

  const arrDestrComputed: any = { x: 1 };
  Object.freeze(arrDestrComputed);
  const destrKey = "x";
  threw = false;
  try {
    [arrDestrComputed[destrKey]] = [7];
  } catch {
    threw = true;
  }
  report("strict frozen [o[k]] = arr:", threw, arrDestrComputed.x);

  // ---- the same `+=` against every rejecting receiver shape ----
  const sealedOwn: any = { x: 1 };
  Object.seal(sealedOwn);
  threw = false;
  try {
    sealedOwn.x += 1;
  } catch {
    threw = true;
  }
  report("strict sealed own +=:", threw, sealedOwn.x);

  // `seal` leaves existing properties WRITABLE, so the line above must succeed
  // in both modes -- it is the over-throw control. A NEW key on a sealed object
  // is the rejecting half.
  const sealedNew: any = { x: 1 };
  Object.seal(sealedNew);
  threw = false;
  try {
    sealedNew.y += 1;
  } catch {
    threw = true;
  }
  report("strict sealed new +=:", threw, hasOwn(sealedNew, "y"));

  const nonWritable = readOnlyOwn(1);
  threw = false;
  try {
    nonWritable.x += 1;
  } catch {
    threw = true;
  }
  report("strict non-writable own +=:", threw, nonWritable.x);

  // See the note in `sloppyArm`: the `+=` spelling on an inherited receiver is
  // a separate, mode-independent defect. `=` is the same three receiver shapes
  // on the lane that already walks the prototype chain.
  const inheritedNonWritable: any = Object.create(nonWritableProto());
  threw = false;
  try {
    inheritedNonWritable.x = 11;
  } catch {
    threw = true;
  }
  report(
    "strict non-writable inherited =:",
    threw,
    hasOwn(inheritedNonWritable, "x"),
    inheritedNonWritable.x,
  );

  const getterOnly = getterOnlyOwn();
  threw = false;
  try {
    getterOnly.x += 1;
  } catch {
    threw = true;
  }
  report("strict getter-only own +=:", threw, getterOnly.x);

  const inheritedGetterOnly: any = Object.create(getterOnlyProto());
  threw = false;
  try {
    inheritedGetterOnly.x = 21;
  } catch {
    threw = true;
  }
  report(
    "strict getter-only inherited =:",
    threw,
    hasOwn(inheritedGetterOnly, "x"),
    inheritedGetterOnly.x,
  );

  // An inherited SETTER runs in both modes and creates no own property: the
  // rejection is about `[[Set]]` returning false, never about reaching the
  // accessor. A fix that routed sloppy stores past the prototype walk would
  // show up here as a missing call.
  const calls: any[] = [];
  const withSetter: any = Object.create(setterProto(calls));
  threw = false;
  try {
    withSetter.x = 31;
  } catch {
    threw = true;
  }
  report("strict inherited setter =:", threw, calls.join(","), hasOwn(withSetter, "x"));

  const noExtend: any = { x: 1 };
  Object.preventExtensions(noExtend);
  threw = false;
  try {
    noExtend.y += 1;
  } catch {
    threw = true;
  }
  report("strict preventExtensions new +=:", threw, hasOwn(noExtend, "y"));

  // preventExtensions leaves existing properties writable -- the second
  // over-throw control.
  const noExtendOwn: any = { x: 1 };
  Object.preventExtensions(noExtendOwn);
  threw = false;
  try {
    noExtendOwn.x += 1;
  } catch {
    threw = true;
  }
  report("strict preventExtensions own +=:", threw, noExtendOwn.x);

  // ---- the class-field store lane ----
  const cell = frozenCell();
  threw = false;
  try {
    cell.x += 1;
  } catch {
    threw = true;
  }
  report("strict frozen class field +=:", threw, cell.x);

  const liveCell = new Cell(1);
  threw = false;
  try {
    liveCell.x += 1;
  } catch {
    threw = true;
  }
  report("strict live class field +=:", threw, liveCell.x);

  // ---- the `arr.length` lane ----
  // `a.length += n` is `Set(O, "length", n, Throw)` on the OBJECT lane, which
  // reaches `js_array_set_length_strict` -- named for the `Throw` it hard-codes.
  const frozenArray: any[] = [1, 2];
  Object.freeze(frozenArray);
  threw = false;
  try {
    frozenArray.length += 1;
  } catch {
    threw = true;
  }
  report("strict frozen array length +=:", threw, frozenArray.length);

  const nonWritableLength: any[] = [1, 2];
  Object.defineProperty(nonWritableLength, "length", { writable: false });
  threw = false;
  try {
    nonWritableLength.length += 1;
  } catch {
    threw = true;
  }
  report("strict non-writable array length +=:", threw, nonWritableLength.length);

  // A live array's `length` accepts the write in both modes: the control that
  // proves the lane still WORKS, not just that it stopped throwing.
  const liveArray: any[] = [1, 2];
  threw = false;
  try {
    liveArray.length += 1;
  } catch {
    threw = true;
  }
  report("strict live array length +=:", threw, liveArray.length);

  // ---- accepted stores, to prove the sloppy tail still STORES ----
  const plain: any = { x: 1 };
  threw = false;
  try {
    plain.x += 41;
  } catch {
    threw = true;
  }
  report("strict plain +=:", threw, plain.x);

  const plainFor: any = { x: 1 };
  threw = false;
  try {
    for (plainFor.x of [7]) {
    }
  } catch {
    threw = true;
  }
  report("strict plain for-of head:", threw, plainFor.x);

  const plainDestr: any = { x: 1 };
  threw = false;
  try {
    [plainDestr.x] = [7];
  } catch {
    threw = true;
  }
  report("strict plain [o.x] = arr:", threw, plainDestr.x);

  const plainNew: any = { x: 1 };
  threw = false;
  try {
    plainNew.y ??= 9;
  } catch {
    threw = true;
  }
  report("strict plain new key ??=:", threw, plainNew.y);

  // ---- `++` (Expr::PropertyUpdate) alongside `+=`, so the two spellings of
  //      one operation are asserted in the same file and mode ----
  const upd: any = { x: 1 };
  Object.freeze(upd);
  threw = false;
  try {
    upd.x++;
  } catch {
    threw = true;
  }
  report("strict frozen ++:", threw, upd.x);

  // ---- and plain `=`, the lane that was already right (#9422) ----
  const assign: any = { x: 1 };
  Object.freeze(assign);
  threw = false;
  try {
    assign.x = 9;
  } catch {
    threw = true;
  }
  report("strict frozen =:", threw, assign.x);
}

sloppyArm();
strictArm();
