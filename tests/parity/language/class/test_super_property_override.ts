// Parity test for issue #774 — value-form `super.<prop>` must resolve
// through the parent's prototype, not the child's instance slot.
//
// Pre-fix (PR #754 lowering): rewrote `super.foo` to `this.foo`, so a
// child override silently shadowed the parent ("B" instead of
// `undefined`).
//
// Strict JS semantics: `super.foo` looks up `foo` on the parent's
// prototype. For an instance-field parent (no method, no getter) the
// prototype slot is empty, so the result is `undefined`. The codegen
// fix in `Expr::SuperPropertyGet` (perry-codegen/src/expr.rs) walks
// the parent class chain explicitly and returns `undefined` here.

class A {
    foo = "A";
}
class B extends A {
    foo = "B";
    parentFoo() {
        return super.foo;
    }
}
console.log(new B().parentFoo());
