// epic #1785 / #1758: a STATIC method call on a class-object VALUE reached
// through the *dynamic* dispatch path must bind `this` to the receiver.
//
// The compile-time static-dispatch tower (property_get.rs ~757) handles a
// `ClassExprFresh` / const-bound class-object receiver. But when the receiver
// is an *un-inlined* factory-call result — e.g. the `extends` clause of
//   `class X extends (make(...) as any).annotations(y) {}`
// (effect's `class BigInt$ extends transformOrFail(...).annotations(...) {}`)
// — the call fell into the instance-method dispatch tower, which passed the
// receiver as arg0 and never set IMPLICIT_THIS. So a static method reading
// `this.<staticField>` (effect's `annotations() { make(this.ast, ...) }`)
// saw `undefined` and threw `Cannot read properties of undefined`.
//
// Fix: route `perry_static_*` implementors through `js_class_static_method_call`
// (codegen tower) and detect class-object receivers in `js_native_call_method`
// (runtime dynamic path) — both bind `this` + walk the class_id parent chain.
//
// Compared byte-for-byte against `node --experimental-strip-types`.

function make(a: any) {
  return class SchemaClass {
    static ast = a;
    static annotations(x: any) {
      // reads `this.ast` (the receiver's static field) — requires `this` bound
      return make({ base: this.ast, ann: x });
    }
  };
}

// (1) extends an inline static-method-call result (tower case0 path: the
//     `make()` receiver's class is statically known, so the class-id switch
//     fires and the case body must use the static calling convention).
class Chained extends (make({ _tag: "C" }) as any).annotations({ id: 1 }) {}
console.log("(1) Chained.ast:", JSON.stringify((Chained as any).ast));

// (2) opaque (any-typed) receiver → the tower can't resolve the class, so the
//     call goes through the runtime dynamic dispatcher `js_native_call_method`.
function opaque(): any {
  return make({ _tag: "O" });
}
class FromOpaque extends (opaque() as any).annotations({ id: 2 }) {}
console.log("(2) FromOpaque.ast:", JSON.stringify((FromOpaque as any).ast));

// (3) a static method that does NOT read `this` still works through the
//     dynamic path (regression guard: don't break the non-this case).
function makeNoThis(a: any) {
  return class S2 {
    static ast = a;
    static fixed(x: any) {
      return make({ fixed: "F", ann: x });
    }
  };
}
class FromNoThis extends (makeNoThis({ _tag: "N" }) as any).fixed({ id: 3 }) {}
console.log("(3) FromNoThis.ast:", JSON.stringify((FromNoThis as any).ast));

// (4) const-bound receiver (the already-working static-dispatch path) — guard
//     that the new routing didn't regress it.
const parent = (make({ _tag: "P" }) as any).annotations({ id: 4 });
class FromConst extends parent {}
console.log("(4) FromConst.ast:", JSON.stringify((FromConst as any).ast));
