// #10482: Object.prototype has no own __proto__ accessor, so
// hasOwnProperty/Object.hasOwn/getOwnPropertyNames/getOwnPropertyDescriptor
// disagree with Node about it, and a `hasOwnProperty.call(Object.prototype,
// key)` prototype-pollution guard (the qs idiom) lets "__proto__" through.
//
// Node has `Object.prototype.__proto__` as a real accessor property:
// { get: [Function], set: [Function], enumerable: false, configurable: true }.

const has = Object.prototype.hasOwnProperty;

// --- Descriptor shape -------------------------------------------------
const d = Object.getOwnPropertyDescriptor(Object.prototype, "__proto__");
console.log(
  "descriptor shape:",
  typeof d?.get,
  typeof d?.set,
  d?.enumerable,
  d?.configurable,
  d ? "value" in d : false,
);

// --- Reflection entry points must agree --------------------------------
console.log("hasOwnProperty.call:", has.call(Object.prototype, "__proto__"));
console.log("Object.hasOwn:", Object.hasOwn(Object.prototype, "__proto__"));
console.log(
  "getOwnPropertyNames includes:",
  Object.getOwnPropertyNames(Object.prototype).includes("__proto__"),
);
console.log(
  "Reflect.ownKeys includes:",
  Reflect.ownKeys(Object.prototype).includes("__proto__"),
);
console.log('"__proto__" in {}:', "__proto__" in {});

// --- Enumerability: accessor is non-enumerable -------------------------
console.log(
  "Object.keys(Object.prototype) excludes it:",
  !Object.keys(Object.prototype).includes("__proto__"),
);
console.log(
  "Object.entries(Object.prototype) excludes it:",
  !Object.entries(Object.prototype).some(([k]) => k === "__proto__"),
);
console.log(
  "propertyIsEnumerable:",
  Object.prototype.propertyIsEnumerable.call(Object.prototype, "__proto__"),
);
{
  let sawIt = false;
  for (const k in {}) {
    if (k === "__proto__") sawIt = true;
  }
  console.log("for-in over {} excludes it:", !sawIt);
}

// --- The qs-style prototype-pollution guard idiom -----------------------
const keys = ["__proto__", "toString", "b"];
console.log(
  "guarded keys (qs idiom):",
  keys.filter((k) => !has.call(Object.prototype, k)).join(","),
);

// --- Behavioural read/write must still work -----------------------------

// Plain object.
{
  const target: any = { inherited: "yes" };
  const plain: any = {};
  plain.__proto__ = target;
  console.log(
    "plain object: read===write target, getPrototypeOf agrees:",
    plain.__proto__ === target,
    Object.getPrototypeOf(plain) === target,
  );
}

// Object.create(null): no legacy setter on the chain, so assignment
// creates an ordinary OWN enumerable data property instead of reparenting.
{
  const target: any = { inherited: "yes" };
  const nullProto: any = Object.create(null);
  nullProto.__proto__ = target;
  console.log(
    "null-proto object: stays null-proto, own data prop, key present:",
    Object.getPrototypeOf(nullProto) === null,
    Object.prototype.hasOwnProperty.call(nullProto, "__proto__"),
    Object.keys(nullProto).join(","),
  );
}

// An own descriptor earlier in the chain shadows the inherited accessor.
{
  const ownProtoData: any = {};
  Object.defineProperty(ownProtoData, "__proto__", {
    value: "before",
    writable: true,
    enumerable: true,
    configurable: true,
  });
  const parentBefore = Object.getPrototypeOf(ownProtoData);
  ownProtoData.__proto__ = "after";
  console.log(
    "own __proto__ data prop shadows the accessor:",
    ownProtoData.__proto__,
    Object.getPrototypeOf(ownProtoData) === parentBefore,
  );
}

// A non-object, non-null RHS is silently ignored (Annex B), not thrown.
{
  const target: any = { inherited: "yes" };
  const assigned: any = {};
  assigned.__proto__ = target;
  assigned.__proto__ = 7;
  console.log(
    "primitive RHS ignored, no throw:",
    Object.getPrototypeOf(assigned) === target,
  );
}

// Declared class instance (CLASS_DECL_PROTOTYPE_OBJECTS).
{
  class Base {}
  class Derived extends Base {}
  const inst = new Derived();
  console.log(
    "declared class instance:",
    (inst as any).__proto__ === Derived.prototype,
    Object.getPrototypeOf(Derived.prototype) === Base.prototype,
  );
}

// Plain-function constructor instance (CLASS_PROTOTYPE_OBJECTS).
{
  function Ctor(this: any) {
    this.x = 1;
  }
  const inst: any = new (Ctor as any)();
  console.log(
    "function-ctor instance:",
    inst.__proto__ === (Ctor as any).prototype,
  );
}

// Object.create(proto) synthetic object (also CLASS_PROTOTYPE_OBJECTS-style
// resolution).
{
  const base = { greet: "hi" };
  const created: any = Object.create(base);
  console.log(
    "Object.create(proto) synthetic object:",
    created.__proto__ === base,
  );
}

// Primitives — auto-boxed to their wrapper's prototype on read.
console.log("number primitive:", (5 as any).__proto__ === Number.prototype);
console.log(
  "string primitive:",
  ("s" as any).__proto__ === String.prototype,
);

// --- Object-literal `__proto__` stays the special non-computed form ------
{
  const litProto = { fromLiteral: true };
  const lit: any = { __proto__: litProto, y: 2 };
  console.log(
    "literal __proto__ sets prototype, not an own key:",
    Object.getPrototypeOf(lit) === litProto,
    !Object.prototype.hasOwnProperty.call(lit, "__proto__"),
    lit.y,
  );
}

// A COMPUTED key that evaluates to "__proto__" is an ordinary own property —
// the special form only applies to the non-computed `__proto__: value` shape.
{
  const key = "__proto__";
  const computed: any = { [key]: 99 };
  console.log(
    "computed __proto__ key is an ordinary own data property:",
    Object.getPrototypeOf(computed) === Object.prototype,
    Object.prototype.hasOwnProperty.call(computed, "__proto__"),
    computed.__proto__,
  );
}
