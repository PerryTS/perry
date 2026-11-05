import { inherits } from "node:util";

function Base() {}
function Sub() {}

function record(label, fn) {
  try {
    console.log(label, fn());
  } catch (error) {
    console.log(label, error.name, error.code, error.message);
  }
}

record("ctor-null:", () => inherits(null, Base));
record("ctor-undefined:", () => inherits(undefined, Base));
record("super-null:", () => inherits(Sub, null));
record("super-undefined:", () => inherits(Sub, undefined));
record("super-object:", () => inherits(Sub, {}));

function NullableSub() {}
const nullableSuper = { prototype: null };
console.log("nullable return:", inherits(NullableSub, nullableSuper));
console.log(
  "nullable result:",
  NullableSub.super_ === nullableSuper,
  Object.getPrototypeOf(NullableSub.prototype) === null,
);
const superDescriptor = Object.getOwnPropertyDescriptor(NullableSub, "super_");
console.log(
  "super descriptor:",
  superDescriptor?.writable,
  superDescriptor?.enumerable,
  superDescriptor?.configurable,
);
