// #12007: lexical bindings initialized by a direct class expression must use
// the module TDZ path before the source-position class-expression binding.
function errorName(fn: () => unknown): string {
  try {
    fn();
    return "no throw";
  } catch (error: any) {
    return error && error.name ? error.name : "other error";
  }
}

const earlyClosure = () => C;
class Earlier {
  static read() { return C; }
}
try {
  const value = C;
  console.log("read-before", "no throw", typeof value);
} catch (error: any) {
  console.log("read-before", error && error.name);
}
try {
  console.log("typeof-before", typeof C);
} catch (error: any) {
  console.log("typeof-before", error && error.name);
}
try {
  new C();
  console.log("new-before", "no throw");
} catch (error: any) {
  console.log("new-before", error && error.name);
}
console.log("closure-before", errorName(() => earlyClosure()));
console.log("method-before", errorName(() => Earlier.read()));

const C = class Named {
  static own = Named;
  static inner() { return Named; }
  static outer() { return C; }
};
console.log("after", typeof C, new C() instanceof C);
console.log("names", C.name, C.own === C, C.inner() === C, C.outer() === C);

{
  const C = class { static self() { return C; } };
  console.log("nested-shadow", C.name, C.self() === C);
}

function shadow(C: any) { return C; }
console.log("shadow", shadow("inner local"));

var V = class {};
function readVar() { return V; }
console.log("var-control", typeof readVar(), V.name, new V() instanceof V);

// The anonymous class's static initializer reads the enclosing lexical
// binding while it is still in its TDZ. Isolate it in a module so the expected
// ReferenceError does not terminate these other controls.
console.log("anonymous-start");
import("./_helpers/issue_12007_anonymous_self_tdz.ts").then(
  () => console.log("anonymous-self", "no throw"),
  (error: any) => console.log("anonymous-self", error && error.name),
);

// The imported function is callable while its exported class-expression
// binding is still uninitialized in the cyclic module.
import { readExportedClass } from "./_helpers/issue_12007_cycle_a.ts";
console.log("cycle-after", typeof readExportedClass());
