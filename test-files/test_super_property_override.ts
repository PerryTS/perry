// Regression net for issue #774 — super.<prop> value-form lowering
// currently rewrites `super.foo` to `this.foo` (see PR #754,
// crates/perry-hir/src/lower/expr_misc.rs lower_super_prop). When the
// child overrides the parent's property, the approximation silently
// returns the child override instead of doing a real super-vtable
// lookup. Strict JS semantics: `super.foo` looks up `foo` on the
// parent prototype, which for an instance field is `undefined`.
//
// Expected (node --experimental-strip-types): undefined
// Actual (perry HEAD):                        B
//
// This test is listed in test-parity/known_failures.json until the
// codegen-side super-vtable path lands; the fix should flip it green.

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
