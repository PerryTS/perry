// Class static blocks run at the source-order position of the
// `class { ... }` declaration, right after the class's static-field
// initializers. Perry hoists classes to a separate `module.classes`
// list, so the lowering injects an inline `StaticMethodCall` for each
// synthetic `__perry_static_init_N` synthetic method at the class-decl
// position in `module.init` — matching the spec's "static initializers
// run during class evaluation" ordering.
//
// Pre-fix the codegen called all static blocks at the *end* of module
// init (after every console.log), so a `class A { static { A.flag =
// true; } } console.log(A.flag);` printed the uninitialized default
// (`0`/`false`) instead of `true`. Refs the `static block initialized:
// true` line of `test_gap_class_advanced.ts`.

// (1) Static block initializing a declared-only static field.
class WithStaticBlock {
  static initialized: boolean;
  static {
    WithStaticBlock.initialized = true;
  }
}
console.log("(1) initialized:", WithStaticBlock.initialized);

// (2) Static block runs AFTER same-class static-field initializers, in
//     source order. `static n = 0` runs first, then `static { A.n = 10
//     }` overrides it; the user-init read should see `10`.
class A {
  static n = 0;
  static {
    A.n = 10;
  }
}
console.log("(2) A.n:", A.n);

// (3) Multiple static blocks in one class run in source order. The
//     second block can read the first's writes.
class D {
  static x = 0;
  static y = 0;
  static {
    D.x = 1;
  }
  static {
    D.y = D.x + 1;
  }
}
console.log("(3) D.x:", D.x, "D.y:", D.y);

// (4) Cross-class block reads see the earlier class's already-run
//     static block result. Classes are evaluated in source order, so
//     by the time C's block runs, A and B have completed their own.
class B {
  static m = 0;
  static {
    B.m = 20;
  }
}
class C {
  static both = 0;
  static {
    C.both = A.n + B.m;
  }
}
console.log("(4) C.both:", C.both);
