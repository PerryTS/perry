// Issue #1789: `new` / `instanceof` / `typeof` on a class-object VALUE — a
// class expression (with static fields, so it lowers to a heap class object
// stamped OBJECT_TYPE_CLASS) bound to a value. Per JS, a class is callable
// (`typeof === "function"`), `new C()` constructs an instance whose methods
// dispatch, and `c instanceof C` is true.
//
// Scope note: per-evaluation class objects share the compile-time template's
// class_id, so cross-evaluation `instanceof` (a `make()` result tested
// against a *different* `make()` result) can't be distinguished by the
// class_id walk — that's inherent to the shared-class_id model and not
// covered here. Constructor-body/field-initializer execution via dynamic
// `new` on a class-object value is a tracked refinement.
//
// Expected output:
// typeof: function
// C.kind: K
// instanceof: true
// method: 42

function make() {
  return class {
    static kind = "K";
    foo() {
      return 42;
    }
  };
}
const C = make();
console.log("typeof:", typeof C);
console.log("C.kind:", (C as any).kind);
const c = new C();
console.log("instanceof:", c instanceof C);
console.log("method:", (c as any).foo());
