// #9495: strict `o.x += 1` against an INHERITED non-writable data property,
// getter-only accessor or setter skipped the prototype walk entirely.
//
// ES2024 SS10.1.9.2 (OrdinarySetWithOwnDescriptor): when the receiver has no
// own property, `[[Set]]` walks to the parent and lets THE PARENT'S descriptor
// decide -- a non-writable data property rejects, a getter-only accessor
// rejects, a setter RUNS with the original receiver, and only a writable data
// property (or the end of the chain) creates a new own property on the
// receiver. `PutValue` then turns a rejection into a TypeError iff the
// reference is strict.
//
// Perry's strict `Expr::PropertySet` tail (`js_typed_feedback_object_set_
// field_by_name_fast` -> `js_object_set_field_by_name`) performed an
// OWN-property store: no walk, so no throw, no setter call, and an own
// property materialised where the spec creates none. The strict object-by-name
// arms of `Expr::IndexSet` (`o["x"] += 1`, `o[k] += 1`) had the same tail.
// `o.x = v` was already right (`Expr::PutValueSet` -> `js_put_value_set`),
// and #9459 made the SLOPPY half of these spellings right as a side effect of
// routing them to that same entry.
//
// This is a missing prototype walk, not a missing `Throw` flag -- it is the
// opposite direction from #9422 and a different defect from #9459 -- so it is
// wrong in BOTH modes on unfixed main, and BOTH ARMS ARE ASSERTED here. The
// sloppy arm distinguishes "silent because sloppy" from "silent because the
// walk never ran": an inherited setter must be CALLED in both modes, and no
// own property may appear in either.
//
// `.cts` so that `sloppyArm` is sloppy and `strictArm` opts in with its own
// directive prologue. The two arms are textual duplicates on purpose: a
// function inherits the strictness of the code it is DEFINED in, never its
// caller's, so a shared helper would test one mode twice.
//
// Companions: test_gap_9459_property_set_strictness.cts (the `Throw` flag on
// these spellings, own-property receivers) and
// test_gap_9422_strict_object_store_strictness.cts (the `=` lane).

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

class Acc {
  calls: any[];
  constructor(calls: any[]) {
    this.calls = calls;
  }
  get x(): number {
    return 40;
  }
  set x(value: number) {
    this.calls.push(value);
  }
}

// An INT32-tagged class ref as the receiver (`Klass.prop`), without a declared
// static field: the store lands in the class's dynamic-property bag. (A
// DECLARED `static n` loses `K.n += 1` in both modes on main -- a static-slot
// lane defect, not a prototype walk -- and is #9526.)
class Bag {}

function sloppyArm(): void {

  let threw = false;
  const calls: any[] = [];

  // ---- `o.x += 1` (Expr::PropertySet) against each inherited receiver ----
  const nw: any = Object.create(nonWritableProto());
  threw = false;
  try {
    nw.x += 1;
  } catch {
    threw = true;
  }
  report("sloppy non-writable inherited +=:", threw, hasOwn(nw, "x"), nw.x);

  const go: any = Object.create(getterOnlyProto());
  threw = false;
  try {
    go.x += 1;
  } catch {
    threw = true;
  }
  report("sloppy getter-only inherited +=:", threw, hasOwn(go, "x"), go.x);

  calls.length = 0;
  const st: any = Object.create(setterProto(calls));
  threw = false;
  try {
    st.x += 1;
  } catch {
    threw = true;
  }
  report("sloppy inherited setter +=:", threw, calls.join(","), hasOwn(st, "x"));

  // Two levels up: the walk must continue past an empty intermediate object.
  calls.length = 0;
  const deep: any = Object.create(Object.create(setterProto(calls)));
  threw = false;
  try {
    deep.x += 1;
  } catch {
    threw = true;
  }
  report("sloppy inherited setter two levels +=:", threw, calls.join(","), hasOwn(deep, "x"));

  // A class accessor on the prototype chain (compiled setter, not a descriptor).
  calls.length = 0;
  const viaClass: any = Object.create(new Acc(calls));
  threw = false;
  try {
    viaClass.x += 1;
  } catch {
    threw = true;
  }
  report("sloppy class accessor inherited +=:", threw, calls.join(","), hasOwn(viaClass, "x"));

  // A Proxy on the prototype chain: `OrdinarySetWithOwnDescriptor` forwards to
  // `parent.[[Set]](P, V, Receiver)` with the ORIGINAL receiver.
  const trapLog: any[] = [];
  const viaProxy: any = Object.create(
    new Proxy(
      { x: 50 },
      {
        set(t: any, k: any, v: any, r: any) {
          trapLog.push(String(k) + "=" + String(v) + ":" + String(r === viaProxy));
          return true;
        },
      },
    ),
  );
  threw = false;
  try {
    viaProxy.x += 1;
  } catch {
    threw = true;
  }
  report("sloppy proxy inherited +=:", threw, trapLog.join(","), hasOwn(viaProxy, "x"));

  // ---- logical assignment: the branch that stores ----
  calls.length = 0;
  const andAnd: any = Object.create(setterProto(calls));
  threw = false;
  try {
    andAnd.x &&= 5;
  } catch {
    threw = true;
  }
  report("sloppy inherited setter &&=:", threw, calls.join(","), hasOwn(andAnd, "x"));

  const goOr: any = Object.create(getterOnlyProto());
  threw = false;
  try {
    goOr.x ??= 5;
  } catch {
    threw = true;
  }
  // getter returns 20, so `??=` never stores: a control that must be silent.
  report("sloppy getter-only inherited ??= (no store):", threw, hasOwn(goOr, "x"), goOr.x);

  // ---- for-of head ----
  calls.length = 0;
  const forHead: any = Object.create(setterProto(calls));
  threw = false;
  try {
    for (forHead.x of [7]) {
    }
  } catch {
    threw = true;
  }
  report("sloppy inherited setter for-of head:", threw, calls.join(","), hasOwn(forHead, "x"));

  const forNw: any = Object.create(nonWritableProto());
  threw = false;
  try {
    for (forNw.x of [7]) {
    }
  } catch {
    threw = true;
  }
  report("sloppy non-writable inherited for-of head:", threw, hasOwn(forNw, "x"), forNw.x);

  // ---- destructuring targets, statement and expression position ----
  calls.length = 0;
  const destr: any = Object.create(setterProto(calls));
  threw = false;
  try {
    [destr.x] = [7];
  } catch {
    threw = true;
  }
  report("sloppy inherited setter [o.x] = arr:", threw, calls.join(","), hasOwn(destr, "x"));

  calls.length = 0;
  const destrExpr: any = Object.create(setterProto(calls));
  threw = false;
  try {
    const seen = ([destrExpr.x] = [7]);
    void seen;
  } catch {
    threw = true;
  }
  report("sloppy inherited setter ([o.x] = arr) expr:", threw, calls.join(","), hasOwn(destrExpr, "x"));

  calls.length = 0;
  const destrObj: any = Object.create(setterProto(calls));
  threw = false;
  try {
    ({ a: destrObj.x } = { a: 7 });
  } catch {
    threw = true;
  }
  report("sloppy inherited setter ({a: o.x}) = obj:", threw, calls.join(","), hasOwn(destrObj, "x"));

  const destrGo: any = Object.create(getterOnlyProto());
  threw = false;
  try {
    [destrGo.x] = [7];
  } catch {
    threw = true;
  }
  report("sloppy getter-only inherited [o.x] = arr:", threw, hasOwn(destrGo, "x"), destrGo.x);

  // ---- computed keys (Expr::IndexSet object-by-name lanes) ----
  calls.length = 0;
  const lit: any = Object.create(setterProto(calls));
  threw = false;
  try {
    lit["x"] += 1;
  } catch {
    threw = true;
  }
  report("sloppy inherited setter o[\"x\"] +=:", threw, calls.join(","), hasOwn(lit, "x"));

  const litNw: any = Object.create(nonWritableProto());
  threw = false;
  try {
    litNw["x"] += 1;
  } catch {
    threw = true;
  }
  report("sloppy non-writable inherited o[\"x\"] +=:", threw, hasOwn(litNw, "x"), litNw.x);

  calls.length = 0;
  const keyed: any = Object.create(setterProto(calls));
  const key = "x";
  threw = false;
  try {
    keyed[key] += 1;
  } catch {
    threw = true;
  }
  report("sloppy inherited setter o[k] +=:", threw, calls.join(","), hasOwn(keyed, "x"));

  const keyedGo: any = Object.create(getterOnlyProto());
  threw = false;
  try {
    keyedGo[key] += 1;
  } catch {
    threw = true;
  }
  report("sloppy getter-only inherited o[k] +=:", threw, hasOwn(keyedGo, "x"), keyedGo.x);

  calls.length = 0;
  const anyKeyed: any = Object.create(setterProto(calls));
  const anyKey: any = "x";
  threw = false;
  try {
    anyKeyed[anyKey] += 1;
  } catch {
    threw = true;
  }
  report("sloppy inherited setter o[anyKey] +=:", threw, calls.join(","), hasOwn(anyKeyed, "x"));

  const anyKeyedNw: any = Object.create(nonWritableProto());
  threw = false;
  try {
    anyKeyedNw[anyKey] += 1;
  } catch {
    threw = true;
  }
  report("sloppy non-writable inherited o[anyKey] +=:", threw, hasOwn(anyKeyedNw, "x"), anyKeyedNw.x);

  calls.length = 0;
  const forKeyed: any = Object.create(setterProto(calls));
  threw = false;
  try {
    for (forKeyed[key] of [7]) {
    }
  } catch {
    threw = true;
  }
  report("sloppy inherited setter for-of head computed:", threw, calls.join(","), hasOwn(forKeyed, "x"));

  calls.length = 0;
  const destrKeyed: any = Object.create(setterProto(calls));
  threw = false;
  try {
    [destrKeyed[key]] = [7];
  } catch {
    threw = true;
  }
  report("sloppy inherited setter [o[k]] = arr:", threw, calls.join(","), hasOwn(destrKeyed, "x"));

  // ---- accepted stores: the tail must still STORE, and still create own
  //      properties where the chain does not object ----
  const plain: any = Object.create({ x: 1 });
  threw = false;
  try {
    plain.x += 41;
  } catch {
    threw = true;
  }
  report("sloppy inherited writable data +=:", threw, hasOwn(plain, "x"), plain.x, Object.getPrototypeOf(plain).x);

  const fresh: any = Object.create(setterProto(calls));
  threw = false;
  try {
    fresh.y ??= 9;
  } catch {
    threw = true;
  }
  report("sloppy new key beside inherited accessor ??=:", threw, hasOwn(fresh, "y"), fresh.y);

  // A class ref as the receiver: the receiver-aware `[[Set]]` must keep
  // routing an INT32-tagged class value to its dynamic-property bag.
  const bag: any = Bag;
  threw = false;
  try {
    bag.sloppy_n = 1;
    bag.sloppy_n += 1;
    bag["sloppy_n"] += 1;
  } catch {
    threw = true;
  }
  report("sloppy class ref +=:", threw, bag.sloppy_n, (Bag as any).sloppy_n);

  // ---- the lanes that were already right, as controls ----
  calls.length = 0;
  const assign: any = Object.create(setterProto(calls));
  threw = false;
  try {
    assign.x = 31;
  } catch {
    threw = true;
  }
  report("sloppy inherited setter =:", threw, calls.join(","), hasOwn(assign, "x"));

  calls.length = 0;
  const upd: any = Object.create(setterProto(calls));
  threw = false;
  try {
    upd.x++;
  } catch {
    threw = true;
  }
  report("sloppy inherited setter ++:", threw, calls.join(","), hasOwn(upd, "x"));

  const updNw: any = Object.create(nonWritableProto());
  threw = false;
  try {
    updNw.x++;
  } catch {
    threw = true;
  }
  report("sloppy non-writable inherited ++:", threw, hasOwn(updNw, "x"), updNw.x);
}

function strictArm(): void {
  "use strict";

  let threw = false;
  const calls: any[] = [];

  // ---- `o.x += 1` (Expr::PropertySet) against each inherited receiver ----
  const nw: any = Object.create(nonWritableProto());
  threw = false;
  try {
    nw.x += 1;
  } catch {
    threw = true;
  }
  report("strict non-writable inherited +=:", threw, hasOwn(nw, "x"), nw.x);

  const go: any = Object.create(getterOnlyProto());
  threw = false;
  try {
    go.x += 1;
  } catch {
    threw = true;
  }
  report("strict getter-only inherited +=:", threw, hasOwn(go, "x"), go.x);

  calls.length = 0;
  const st: any = Object.create(setterProto(calls));
  threw = false;
  try {
    st.x += 1;
  } catch {
    threw = true;
  }
  report("strict inherited setter +=:", threw, calls.join(","), hasOwn(st, "x"));

  // Two levels up: the walk must continue past an empty intermediate object.
  calls.length = 0;
  const deep: any = Object.create(Object.create(setterProto(calls)));
  threw = false;
  try {
    deep.x += 1;
  } catch {
    threw = true;
  }
  report("strict inherited setter two levels +=:", threw, calls.join(","), hasOwn(deep, "x"));

  // A class accessor on the prototype chain (compiled setter, not a descriptor).
  calls.length = 0;
  const viaClass: any = Object.create(new Acc(calls));
  threw = false;
  try {
    viaClass.x += 1;
  } catch {
    threw = true;
  }
  report("strict class accessor inherited +=:", threw, calls.join(","), hasOwn(viaClass, "x"));

  // A Proxy on the prototype chain: `OrdinarySetWithOwnDescriptor` forwards to
  // `parent.[[Set]](P, V, Receiver)` with the ORIGINAL receiver.
  const trapLog: any[] = [];
  const viaProxy: any = Object.create(
    new Proxy(
      { x: 50 },
      {
        set(t: any, k: any, v: any, r: any) {
          trapLog.push(String(k) + "=" + String(v) + ":" + String(r === viaProxy));
          return true;
        },
      },
    ),
  );
  threw = false;
  try {
    viaProxy.x += 1;
  } catch {
    threw = true;
  }
  report("strict proxy inherited +=:", threw, trapLog.join(","), hasOwn(viaProxy, "x"));

  // ---- logical assignment: the branch that stores ----
  calls.length = 0;
  const andAnd: any = Object.create(setterProto(calls));
  threw = false;
  try {
    andAnd.x &&= 5;
  } catch {
    threw = true;
  }
  report("strict inherited setter &&=:", threw, calls.join(","), hasOwn(andAnd, "x"));

  const goOr: any = Object.create(getterOnlyProto());
  threw = false;
  try {
    goOr.x ??= 5;
  } catch {
    threw = true;
  }
  // getter returns 20, so `??=` never stores: a control that must be silent.
  report("strict getter-only inherited ??= (no store):", threw, hasOwn(goOr, "x"), goOr.x);

  // ---- for-of head ----
  calls.length = 0;
  const forHead: any = Object.create(setterProto(calls));
  threw = false;
  try {
    for (forHead.x of [7]) {
    }
  } catch {
    threw = true;
  }
  report("strict inherited setter for-of head:", threw, calls.join(","), hasOwn(forHead, "x"));

  const forNw: any = Object.create(nonWritableProto());
  threw = false;
  try {
    for (forNw.x of [7]) {
    }
  } catch {
    threw = true;
  }
  report("strict non-writable inherited for-of head:", threw, hasOwn(forNw, "x"), forNw.x);

  // ---- destructuring targets, statement and expression position ----
  calls.length = 0;
  const destr: any = Object.create(setterProto(calls));
  threw = false;
  try {
    [destr.x] = [7];
  } catch {
    threw = true;
  }
  report("strict inherited setter [o.x] = arr:", threw, calls.join(","), hasOwn(destr, "x"));

  calls.length = 0;
  const destrExpr: any = Object.create(setterProto(calls));
  threw = false;
  try {
    const seen = ([destrExpr.x] = [7]);
    void seen;
  } catch {
    threw = true;
  }
  report("strict inherited setter ([o.x] = arr) expr:", threw, calls.join(","), hasOwn(destrExpr, "x"));

  calls.length = 0;
  const destrObj: any = Object.create(setterProto(calls));
  threw = false;
  try {
    ({ a: destrObj.x } = { a: 7 });
  } catch {
    threw = true;
  }
  report("strict inherited setter ({a: o.x}) = obj:", threw, calls.join(","), hasOwn(destrObj, "x"));

  const destrGo: any = Object.create(getterOnlyProto());
  threw = false;
  try {
    [destrGo.x] = [7];
  } catch {
    threw = true;
  }
  report("strict getter-only inherited [o.x] = arr:", threw, hasOwn(destrGo, "x"), destrGo.x);

  // ---- computed keys (Expr::IndexSet object-by-name lanes) ----
  calls.length = 0;
  const lit: any = Object.create(setterProto(calls));
  threw = false;
  try {
    lit["x"] += 1;
  } catch {
    threw = true;
  }
  report("strict inherited setter o[\"x\"] +=:", threw, calls.join(","), hasOwn(lit, "x"));

  const litNw: any = Object.create(nonWritableProto());
  threw = false;
  try {
    litNw["x"] += 1;
  } catch {
    threw = true;
  }
  report("strict non-writable inherited o[\"x\"] +=:", threw, hasOwn(litNw, "x"), litNw.x);

  calls.length = 0;
  const keyed: any = Object.create(setterProto(calls));
  const key = "x";
  threw = false;
  try {
    keyed[key] += 1;
  } catch {
    threw = true;
  }
  report("strict inherited setter o[k] +=:", threw, calls.join(","), hasOwn(keyed, "x"));

  const keyedGo: any = Object.create(getterOnlyProto());
  threw = false;
  try {
    keyedGo[key] += 1;
  } catch {
    threw = true;
  }
  report("strict getter-only inherited o[k] +=:", threw, hasOwn(keyedGo, "x"), keyedGo.x);

  calls.length = 0;
  const anyKeyed: any = Object.create(setterProto(calls));
  const anyKey: any = "x";
  threw = false;
  try {
    anyKeyed[anyKey] += 1;
  } catch {
    threw = true;
  }
  report("strict inherited setter o[anyKey] +=:", threw, calls.join(","), hasOwn(anyKeyed, "x"));

  const anyKeyedNw: any = Object.create(nonWritableProto());
  threw = false;
  try {
    anyKeyedNw[anyKey] += 1;
  } catch {
    threw = true;
  }
  report("strict non-writable inherited o[anyKey] +=:", threw, hasOwn(anyKeyedNw, "x"), anyKeyedNw.x);

  calls.length = 0;
  const forKeyed: any = Object.create(setterProto(calls));
  threw = false;
  try {
    for (forKeyed[key] of [7]) {
    }
  } catch {
    threw = true;
  }
  report("strict inherited setter for-of head computed:", threw, calls.join(","), hasOwn(forKeyed, "x"));

  calls.length = 0;
  const destrKeyed: any = Object.create(setterProto(calls));
  threw = false;
  try {
    [destrKeyed[key]] = [7];
  } catch {
    threw = true;
  }
  report("strict inherited setter [o[k]] = arr:", threw, calls.join(","), hasOwn(destrKeyed, "x"));

  // ---- accepted stores: the tail must still STORE, and still create own
  //      properties where the chain does not object ----
  const plain: any = Object.create({ x: 1 });
  threw = false;
  try {
    plain.x += 41;
  } catch {
    threw = true;
  }
  report("strict inherited writable data +=:", threw, hasOwn(plain, "x"), plain.x, Object.getPrototypeOf(plain).x);

  const fresh: any = Object.create(setterProto(calls));
  threw = false;
  try {
    fresh.y ??= 9;
  } catch {
    threw = true;
  }
  report("strict new key beside inherited accessor ??=:", threw, hasOwn(fresh, "y"), fresh.y);

  // A class ref as the receiver: the receiver-aware `[[Set]]` must keep
  // routing an INT32-tagged class value to its dynamic-property bag.
  const bag: any = Bag;
  threw = false;
  try {
    bag.strict_n = 1;
    bag.strict_n += 1;
    bag["strict_n"] += 1;
  } catch {
    threw = true;
  }
  report("strict class ref +=:", threw, bag.strict_n, (Bag as any).strict_n);

  // ---- the lanes that were already right, as controls ----
  calls.length = 0;
  const assign: any = Object.create(setterProto(calls));
  threw = false;
  try {
    assign.x = 31;
  } catch {
    threw = true;
  }
  report("strict inherited setter =:", threw, calls.join(","), hasOwn(assign, "x"));

  calls.length = 0;
  const upd: any = Object.create(setterProto(calls));
  threw = false;
  try {
    upd.x++;
  } catch {
    threw = true;
  }
  report("strict inherited setter ++:", threw, calls.join(","), hasOwn(upd, "x"));

  const updNw: any = Object.create(nonWritableProto());
  threw = false;
  try {
    updNw.x++;
  } catch {
    threw = true;
  }
  report("strict non-writable inherited ++:", threw, hasOwn(updNw, "x"), updNw.x);
}

sloppyArm();
strictArm();
