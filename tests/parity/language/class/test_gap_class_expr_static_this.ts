// Issue #1787: a static method called on a class-object VALUE (a class
// expression returned from a factory, lowered to a heap class object) must
// bind `this` to that value, so `this.<field>` reads this evaluation's own
// static field — not the shared template's static-field global. Both the
// inline factory-result form (`make(a).m()`) and the const-bound form
// (`const C = make(a); C.m()`) dispatch through the template's static chain
// with `this` = the actual object.
//
// Scope: the static-method CALL binding (acceptance criterion #1). The
// method VALUE-read form (`typeof make(a).m === "function"`, criterion #2)
// goes through the separate value-read path and is tracked as the remaining
// #1787 sub-item.
//
// Expected output:
// inline X: X
// inline Y: Y
// const Z: Z
// two-arg: a|b X

function make(tag: string) {
  return class {
    static ast = tag;
    static viaThis() {
      return this.ast;
    }
    static withArgs(a: string, b: string) {
      return a + "|" + b + " " + this.ast;
    }
  };
}

// Inline factory-result static-method call binds `this` to the fresh object.
console.log("inline X:", make("X").viaThis());
console.log("inline Y:", make("Y").viaThis());

// Const-bound class object: `C.viaThis()` binds `this` to C, reading C's
// own per-evaluation `ast`.
const C = make("Z");
console.log("const Z:", C.viaThis());

// Args are not corrupted by the receiver (static methods have no `this`
// param; `this` arrives via the implicit-this slot).
console.log("two-arg:", make("X").withArgs("a", "b"));
