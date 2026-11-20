// #9394: a failed indexed [[Set]] on an Array throws ONLY in strict mode.
//
// ES2024 SS6.2.5.7 (PutValue) calls `Set(O, P, V, Throw)` with
// Throw = IsStrictReference(ref). A rejected write is therefore a silent no-op
// in sloppy code and a TypeError in strict code -- for arrays exactly as for
// ordinary objects. #9326 ("indexed writes honour a custom array prototype",
// live again via #9370) routed the cold element-store continuation through the
// STRICT runtime entry unconditionally, so every rejected array element write
// began throwing regardless of the assignment's own strictness. Plain objects
// were unaffected, which is why a 64-check differential and a 205-line gap
// fixture -- all of it module (strict) code -- stayed green.
//
// This file is `.cts`, so it is a CommonJS script in BOTH runtimes: `sloppyArm`
// is sloppy code and `strictArm` opts in with its own directive prologue.
// ASSERTING ONLY THE THROW IS WHAT LET THIS THROUGH, so every case below is
// asserted in both modes.
//
// The ordinary-object control appears in the sloppy arm only. Perry emits
// `js_put_value_set(..., strict = 0)` at every property-set site, so a rejected
// STRICT ordinary-object write is silent too -- a separate, pre-existing gap
// that is not what #9394 is about and is not fixed here.
//
// The two arms are textual duplicates on purpose: a function inherits the
// strictness of the code it is DEFINED in, never its caller's, so a shared
// helper would test sloppy twice. Only the mode prefix differs.

function report(name: string, threw: boolean, ...rest: unknown[]): void {
  console.log(name, threw ? "TypeError" : "silent", ...rest);
}

function hasOwn(value: any, key: PropertyKey): boolean {
  return Object.prototype.hasOwnProperty.call(value, key);
}

function lockedProto(): any {
  const proto: any = {};
  Object.defineProperty(proto, "6", {
    configurable: true,
    enumerable: true,
    value: "lockedSix",
    writable: false,
  });
  return proto;
}

function getterOnlyProto(): any {
  const proto: any = {};
  Object.defineProperty(proto, "4", {
    configurable: true,
    get() {
      return "getterOnly4";
    },
  });
  return proto;
}

function setterProto(calls: any[]): any {
  const proto: any = {};
  Object.defineProperty(proto, "4", {
    configurable: true,
    get() {
      return "acc4";
    },
    set(value: any) {
      calls.push(value);
    },
  });
  return proto;
}

function sloppyArm(): void {
  let threw = false;

  const frozen: any[] = [1, 2, 3];
  Object.freeze(frozen);
  threw = false;
  try {
    frozen[0] = 9;
  } catch {
    threw = true;
  }
  report("sloppy frozen in-bounds:", threw, frozen[0], frozen.length);

  const frozenOob: any[] = [1];
  Object.freeze(frozenOob);
  threw = false;
  try {
    frozenOob[5] = 9;
  } catch {
    threw = true;
  }
  report("sloppy frozen new-index:", threw, frozenOob[5], frozenOob.length);

  const readOnly: any[] = [1];
  Object.defineProperty(readOnly, 0, { writable: false });
  threw = false;
  try {
    readOnly[0] = 9;
  } catch {
    threw = true;
  }
  report("sloppy non-writable own index:", threw, readOnly[0], readOnly.length);

  const noExtend: any[] = [1];
  Object.preventExtensions(noExtend);
  threw = false;
  try {
    noExtend[5] = 9;
  } catch {
    threw = true;
  }
  report("sloppy preventExtensions new-index:", threw, noExtend[5], noExtend.length);

  // Sealed leaves existing elements writable, so this succeeds in both modes.
  const sealed: any[] = [1];
  Object.seal(sealed);
  threw = false;
  try {
    sealed[0] = 9;
  } catch {
    threw = true;
  }
  report("sloppy sealed in-bounds:", threw, sealed[0], sealed.length);

  // Control: the ordinary-object [[Set]] path, which already honours Throw.
  const obj: any = { x: 1 };
  Object.freeze(obj);
  threw = false;
  try {
    obj.x = 9;
  } catch {
    threw = true;
  }
  report("sloppy frozen plain object:", threw, obj.x);

  // #9220 shapes: an inherited index is consulted before an own element is
  // created. Rejecting it is exactly what has to become mode-sensitive.
  const lockedTarget: any = [1];
  Object.setPrototypeOf(lockedTarget, lockedProto());
  threw = false;
  try {
    lockedTarget[6] = "changed";
  } catch {
    threw = true;
  }
  report(
    "sloppy inherited non-writable:",
    threw,
    hasOwn(lockedTarget, 6),
    lockedTarget[6],
    lockedTarget.length,
  );

  const getterOnlyTarget: any = [1];
  Object.setPrototypeOf(getterOnlyTarget, getterOnlyProto());
  threw = false;
  try {
    getterOnlyTarget[4] = "changed";
  } catch {
    threw = true;
  }
  report(
    "sloppy inherited getter-only:",
    threw,
    hasOwn(getterOnlyTarget, 4),
    getterOnlyTarget[4],
    getterOnlyTarget.length,
  );

  // An inherited setter runs in both modes and creates no own element.
  const calls: any[] = [];
  const setterTarget: any = [1];
  Object.setPrototypeOf(setterTarget, setterProto(calls));
  threw = false;
  try {
    setterTarget[4] = 11;
  } catch {
    threw = true;
  }
  report(
    "sloppy inherited setter:",
    threw,
    calls.join(","),
    hasOwn(setterTarget, 4),
    setterTarget.length,
  );
}

function strictArm(): void {
  "use strict";

  let threw = false;

  const frozen: any[] = [1, 2, 3];
  Object.freeze(frozen);
  threw = false;
  try {
    frozen[0] = 9;
  } catch {
    threw = true;
  }
  report("strict frozen in-bounds:", threw, frozen[0], frozen.length);

  const frozenOob: any[] = [1];
  Object.freeze(frozenOob);
  threw = false;
  try {
    frozenOob[5] = 9;
  } catch {
    threw = true;
  }
  report("strict frozen new-index:", threw, frozenOob[5], frozenOob.length);

  const readOnly: any[] = [1];
  Object.defineProperty(readOnly, 0, { writable: false });
  threw = false;
  try {
    readOnly[0] = 9;
  } catch {
    threw = true;
  }
  report("strict non-writable own index:", threw, readOnly[0], readOnly.length);

  const noExtend: any[] = [1];
  Object.preventExtensions(noExtend);
  threw = false;
  try {
    noExtend[5] = 9;
  } catch {
    threw = true;
  }
  report("strict preventExtensions new-index:", threw, noExtend[5], noExtend.length);

  // Sealed leaves existing elements writable, so this succeeds in both modes.
  const sealed: any[] = [1];
  Object.seal(sealed);
  threw = false;
  try {
    sealed[0] = 9;
  } catch {
    threw = true;
  }
  report("strict sealed in-bounds:", threw, sealed[0], sealed.length);

  // #9220 shapes: an inherited index is consulted before an own element is
  // created. Rejecting it is exactly what has to become mode-sensitive.
  const lockedTarget: any = [1];
  Object.setPrototypeOf(lockedTarget, lockedProto());
  threw = false;
  try {
    lockedTarget[6] = "changed";
  } catch {
    threw = true;
  }
  report(
    "strict inherited non-writable:",
    threw,
    hasOwn(lockedTarget, 6),
    lockedTarget[6],
    lockedTarget.length,
  );

  const getterOnlyTarget: any = [1];
  Object.setPrototypeOf(getterOnlyTarget, getterOnlyProto());
  threw = false;
  try {
    getterOnlyTarget[4] = "changed";
  } catch {
    threw = true;
  }
  report(
    "strict inherited getter-only:",
    threw,
    hasOwn(getterOnlyTarget, 4),
    getterOnlyTarget[4],
    getterOnlyTarget.length,
  );

  // An inherited setter runs in both modes and creates no own element.
  const calls: any[] = [];
  const setterTarget: any = [1];
  Object.setPrototypeOf(setterTarget, setterProto(calls));
  threw = false;
  try {
    setterTarget[4] = 11;
  } catch {
    threw = true;
  }
  report(
    "strict inherited setter:",
    threw,
    calls.join(","),
    hasOwn(setterTarget, 4),
    setterTarget.length,
  );
}

sloppyArm();
strictArm();
