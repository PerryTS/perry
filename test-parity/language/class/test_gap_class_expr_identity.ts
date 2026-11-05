// Issue #1772 / #1786 (foundation): a class EXPRESSION must evaluate to a
// real heap "class object" with PER-EVALUATION identity and its own static
// fields — the "factory returns a class" pattern (e.g. effect Schema's
// `make(ast) => class { static ast = ast }`). Perry previously hoisted a
// class expression to ONE shared class id, so `make(a) === make(b)` and
// every returned class shared (clobbered/undefined) statics. Now each
// evaluation allocates a distinct class object stamped with the compile-time
// template's class_id, carrying its static fields as OWN properties — so the
// pointers differ and `.tag` is per-evaluation, while a no-`this` static
// method still dispatches via the template class_id.
//
// Scope note: `extends`-inheritance of per-evaluation statics (#1788),
// static-method `this`-binding (#1787) and new/instanceof on a class-object
// value (#1789) are later phases and covered by their own tests. Reading a
// per-eval static back through a binding the compiler resolves to the
// template (`const X = class { static v = 1 }; X.v`) is also a later phase
// — it requires dispatching the property read dynamically on the value
// rather than as a compile-time static-field-get on the template — so the
// cases below read through factory results, whose type is opaque to the
// compiler and therefore already resolve dynamically on the value.
//
// Expected output:
// identity: false
// A.tag B.tag: AAA BBB
// greet: hi
// anon static: 77

function make(tag: string) {
  return class C {
    static tag = tag;
    static greet() {
      return "hi";
    }
  };
}
const A = make("AAA");
const B = make("BBB");
// Distinct heap allocations → distinct pointers.
console.log("identity:", (A as any) === (B as any));
// Per-evaluation own static fields.
console.log("A.tag B.tag:", (A as any).tag, (B as any).tag);
// A no-`this` static method still dispatches (via class_id = template).
console.log("greet:", (A as any).greet());

// An anonymous class expression (no `C` name) also gets its own statics.
function makeAnon(n: number) {
  return class {
    static v = n;
  };
}
console.log("anon static:", (makeAnon(77) as any).v);
