// Issue #263 — arrow function stored as a class field crashes (SIGSEGV) when
// the body reads `this`.
//
// Pre-fix: arrow-function class field initializers were hoisted into the
// constructor via `apply_field_initializers_recursive`, but the closure's
// reserved `this` capture slot (index `auto_captures.len()`) was never
// patched with the constructor's `this`. The slot stayed at 0.0 (the
// initial sentinel `Expr::Closure` codegen writes), and any `this.x` read
// inside the arrow body dereferenced address 0 → SIGSEGV.
//
// Post-fix: `apply_field_initializers_recursive` now mirrors the same
// patch-after-build pattern `lower_object_literal` uses for object-literal
// methods — when an init expression is `Expr::Closure { captures_this:
// true, .. }`, lower the closure, patch its reserved this-slot with the
// current `this`, then store the closure as the field. The arrow's body
// now reads the real instance pointer.

class Foo {
  public value = 99;
  readonly arrowField = () => this.value;
  readonly arrowWithArg = (n: number) => this.value + n;
}

const foo = new Foo();
console.log("foo.value:", foo.value);
console.log("typeof arrow:", typeof foo.arrowField);
console.log("arrowField():", foo.arrowField());
console.log("arrowWithArg(1):", foo.arrowWithArg(1));

// Two instances must each see their own `this` — the closure's capture slot
// is per-instance because each `new Foo()` runs its own field-init pass.
class Counter {
  count = 0;
  inc = () => { this.count++; return this.count; };
}
const a = new Counter();
const b = new Counter();
a.inc(); a.inc(); a.inc();
b.inc();
console.log("a.count:", a.count);
console.log("b.count:", b.count);
