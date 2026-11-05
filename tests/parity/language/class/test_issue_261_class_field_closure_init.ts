// Regression for #261: class field initializer with an arrow-function
// closure literal must compile (no clang "use of undefined value
// '@perry_closure_*'" error). Pre-fix, the closure body was never
// emitted because `collect_closures_in_stmts` walked methods/ctors but
// skipped `class.fields[i].init`, even though
// `apply_field_initializers_recursive` hoists those inits into the
// constructor and emits a `js_closure_alloc(@perry_closure_*)`
// reference.
//
// This test exercises the compile-time fix only. It deliberately avoids
// the `(n) => this.double(n)` shape (closure inside `new X(closure)`
// that lexically captures `this`) — that hits an unrelated
// closure/this-resolution runtime bug whose fix is out of scope here.

class Inner {
  cb: (n: number) => number;
  constructor(cb: (n: number) => number) {
    this.cb = cb;
  }
  run(x: number): number {
    return this.cb(x);
  }
}

class Outer {
  // Direct arrow init.
  private double = (n: number) => n * 2;

  // Arrow init inside a `new` call (no `this` capture). This is the
  // exact shape that triggered #261's "undefined value" link error.
  private inner = new Inner((n) => n + 100);

  // Arrow init inside an object literal (matches the #261 repro's
  // _commandCtx field shape).
  private ops = {
    triple: (n: number) => n * 3,
    quad: (n: number) => n * 4,
  };

  computeDouble(x: number): number {
    return this.double(x);
  }
  computeInner(x: number): number {
    return this.inner.run(x);
  }
  computeTriple(x: number): number {
    return this.ops.triple(x);
  }
  computeQuad(x: number): number {
    return this.ops.quad(x);
  }
}

const o = new Outer();
console.log(o.computeDouble(5));
console.log(o.computeInner(5));
console.log(o.computeTriple(5));
console.log(o.computeQuad(5));
