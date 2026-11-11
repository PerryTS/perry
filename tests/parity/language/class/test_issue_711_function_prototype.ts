// Issue #711 part 2: `function Base() {}; Base.prototype = X` pattern.
// Effect's `internal/effectable.ts` uses this to declare classes via
// prototype-object assignment on a plain function. To make
// `class Derived extends Base {}` walk into the prototype object's
// methods at dispatch time, Perry models this as a synthetic class:
// `js_set_function_prototype` allocates a synthetic class_id keyed by
// the function value and binds the proto object as its vtable source.
// `js_register_class_parent_dynamic` detects closure parent values
// and wires the synthetic class_id into CLASS_REGISTRY. Method
// dispatch (via `js_object_get_field_by_name`) walks the class chain
// and resolves the method as an own-property of the proto object.

const proto = {
  pipe(): string {
    return "proto.pipe";
  },
  greet(): string {
    return "proto.greet";
  },
};

// Shape 1: bare function, prototype-assigned, then extended.
function MyFn() {}
(MyFn as any).prototype = proto;
class Derived extends (MyFn as any) {}

// Shape 2: Effect's exact pattern — IIFE that declares a function and
// returns it.
const Base = (function () {
  const innerProto = {
    pipe(): string {
      return "iife.pipe";
    },
  };
  function Base() {}
  (Base as any).prototype = innerProto;
  return Base as any;
})();
class IIFEDerived extends Base {}

console.log("derived.pipe:", new Derived().pipe());
console.log("derived.greet:", new Derived().greet());
console.log("iife_derived.pipe:", new IIFEDerived().pipe());
