// #10599: `class Sub extends EventEmitter {}` left
// `Object.getPrototypeOf(Sub.prototype) !== EventEmitter.prototype`. Split out
// from #10556 (PR #10592 fixed `new Sub() instanceof EventEmitter`, a
// different mechanism — the class-chain parent edge — that does not depend on
// prototype-OBJECT identity). Root cause: `class_decl_prototype_value`
// recurses on a registered parent class id via itself, which bails for a
// RESERVED native-builtin id (no `js_register_class_name` registration), so
// the link silently fell through to `Object.prototype`.
import { EventEmitter, EventEmitterAsyncResource } from "node:events";

// --- direct subclass, with a field + constructor -----------------------------------
class Sub extends EventEmitter {
  tag = "sub";
  constructor() {
    super();
    this.tag = "sub-ctor";
  }
}

console.log(
  "getPrototypeOf(Sub.prototype) === EventEmitter.prototype:",
  Object.getPrototypeOf(Sub.prototype) === EventEmitter.prototype,
);
console.log("typeof EventEmitter.prototype:", typeof EventEmitter.prototype);
console.log("Sub.prototype instanceof EventEmitter:", Sub.prototype instanceof EventEmitter);
console.log("new Sub() instanceof EventEmitter:", new Sub() instanceof EventEmitter);
console.log(
  "getPrototypeOf(EventEmitter.prototype) === Object.prototype:",
  Object.getPrototypeOf(EventEmitter.prototype) === Object.prototype,
);
console.log(
  "getPrototypeOf(new Sub()) === Sub.prototype:",
  Object.getPrototypeOf(new Sub()) === Sub.prototype,
);

// --- fieldless subclass, no constructor --------------------------------------------
class Fieldless extends EventEmitter {}
console.log(
  "fieldless getPrototypeOf(Fieldless.prototype) === EventEmitter.prototype:",
  Object.getPrototypeOf(Fieldless.prototype) === EventEmitter.prototype,
);
console.log("new Fieldless() instanceof EventEmitter:", new Fieldless() instanceof EventEmitter);

// --- two-level subclass --------------------------------------------------------------
class Mid extends EventEmitter {
  mid = true;
}
class Grandchild extends Mid {
  gc = true;
}
console.log(
  "getPrototypeOf(Mid.prototype) === EventEmitter.prototype:",
  Object.getPrototypeOf(Mid.prototype) === EventEmitter.prototype,
);
console.log(
  "getPrototypeOf(Grandchild.prototype) === Mid.prototype:",
  Object.getPrototypeOf(Grandchild.prototype) === Mid.prototype,
);
console.log(
  "getPrototypeOf(getPrototypeOf(Grandchild.prototype)) === EventEmitter.prototype:",
  Object.getPrototypeOf(Object.getPrototypeOf(Grandchild.prototype)) === EventEmitter.prototype,
);
const gc = new Grandchild();
console.log("grandchild instanceof EventEmitter:", gc instanceof EventEmitter);
console.log("grandchild instanceof Mid:", gc instanceof Mid);
console.log(
  "getPrototypeOf(new Grandchild()) === Grandchild.prototype:",
  Object.getPrototypeOf(new Grandchild()) === Grandchild.prototype,
);

// --- class expression -----------------------------------------------------------------
const ExprSub = class extends EventEmitter {
  expr = true;
};
console.log(
  "expr getPrototypeOf(ExprSub.prototype) === EventEmitter.prototype:",
  Object.getPrototypeOf(ExprSub.prototype) === EventEmitter.prototype,
);
console.log("new ExprSub() instanceof EventEmitter:", new ExprSub() instanceof EventEmitter);

// An UNNAMED, fieldless class expression — the exact minimal shape (no
// literal top-level `class Foo extends EventEmitter {}` declaration to key
// off of, no fields, no constructor).
const Sub3 = class extends EventEmitter {};
console.log(
  "unnamed fieldless getPrototypeOf(Sub3.prototype) === EventEmitter.prototype:",
  Object.getPrototypeOf(Sub3.prototype) === EventEmitter.prototype,
);
console.log("new Sub3() instanceof EventEmitter:", new Sub3() instanceof EventEmitter);

// A named class expression, assigned through an extra indirection (so codegen
// cannot special-case a literal top-level `class Foo extends EventEmitter {}`
// declaration shape).
function makeSubclass() {
  return class NamedExpr extends EventEmitter {};
}
const IndirectSub = makeSubclass();
console.log(
  "indirect getPrototypeOf(IndirectSub.prototype) === EventEmitter.prototype:",
  Object.getPrototypeOf(IndirectSub.prototype) === EventEmitter.prototype,
);

// --- EventEmitterAsyncResource variant --------------------------------------------------
class SubAsync extends EventEmitterAsyncResource {}
console.log(
  "getPrototypeOf(SubAsync.prototype) === EventEmitterAsyncResource.prototype:",
  Object.getPrototypeOf(SubAsync.prototype) === EventEmitterAsyncResource.prototype,
);
// NOT covered here: `Object.getPrototypeOf(EventEmitterAsyncResource.prototype)
// === EventEmitter.prototype` -- that is EventEmitterAsyncResource's OWN
// internal chain (a native-builtin-to-native-builtin link set up wherever its
// `.prototype` is first materialized), not a user `extends` subclass. It is a
// separate, pre-existing gap (Perry answers `false`, Node `true`) outside
// this fix's scope -- `class_decl_prototype_value` never runs for it at all,
// since EventEmitterAsyncResource itself has no declared-class registration.
console.log(
  "SubAsync.prototype instanceof EventEmitterAsyncResource:",
  SubAsync.prototype instanceof EventEmitterAsyncResource,
);

// --- control: a builtin whose subclass instance/prototype modeling is NOT this
// fallback (Array has its own dedicated ArrayHeader-based path) — guards the
// "Array/Map/Set/Error/typed-array subclasses don't reach this fallback"
// claim in the fix's own reasoning so a future change that breaks it fails
// loudly here instead of silently.
class ArraySub extends Array {}
console.log(
  "getPrototypeOf(ArraySub.prototype) === Array.prototype:",
  Object.getPrototypeOf(ArraySub.prototype) === Array.prototype,
);

// --- `in` / `for...in` — the two-prototype-path weakness CLAUDE.md calls out ----------
// (CLASS_PROTOTYPE_OBJECTS vs CLASS_DECL_PROTOTYPE_OBJECTS disagreeing about
// the same chain). `in` and `for...in` walk the exact same
// `[[Prototype]]` link `Object.getPrototypeOf` does, so fixing the identity
// above must also fix these without a separate code path.
console.log("'on' in Sub.prototype:", "on" in Sub.prototype);
console.log("'on' in new Sub():", "on" in new Sub());
console.log("'emit' in new Sub():", "emit" in new Sub());
console.log("'addListener' in new Sub():", "addListener" in new Sub());

const forInKeys: string[] = [];
for (const k in new Sub()) forInKeys.push(k);
console.log("for...in includes 'on':", forInKeys.includes("on"));
console.log("for...in includes 'emit':", forInKeys.includes("emit"));
console.log("for...in includes own field 'tag':", forInKeys.includes("tag"));

// NOT covered here: `Object.keys(new Sub())`. It diverges from Node
// regardless of this fix (verified identically wrong with the fix reverted):
// Perry's native-base `super()` handling installs EventEmitter's methods
// (`on`, `emit`, ...) as literal OWN enumerable properties on the instance
// (CLAUDE.md "Known-weak areas: Native base-class subclassing -- a native
// base's surface is installed at super() time"), and never sets the
// `_events`/`_eventsCount`/`_maxListeners` own fields Node's real
// EventEmitter constructor does. That is an own-property-enumeration defect,
// orthogonal to the [[Prototype]] CHAIN identity this fix corrects.

// --- the emitter still works (identity link must not disturb dispatch) ---------------
const s = new Sub();
let fired = 0;
s.on("ping", (n: number) => (fired += n));
s.emit("ping", 4);
console.log("sub fired:", fired, "listeners:", s.listenerCount("ping"));
console.log("sub.tag:", s.tag);

// --- own-key enumeration on EventEmitter.prototype survives identity too -------------
console.log(
  "EventEmitter.prototype.constructor === EventEmitter:",
  (EventEmitter.prototype as any).constructor === EventEmitter,
);
