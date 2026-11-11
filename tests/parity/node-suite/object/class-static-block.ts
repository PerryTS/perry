// Issue #2278 — `static { ... }` blocks on a class declaration must
// run at the class declaration's source position (per ES spec), not
// at the end of module init. Pre-fix every read after the class
// declaration saw the field's zero default because Perry deferred the
// static-block call to `init_static_fields_late` (after every
// module-level stmt). Surfaced by bisecting `test_gap_class_advanced.ts`
// for #1635 — the single residual failure.

// Basic case from the issue: declared-typed boolean field, assigned in
// a static block, read in the next module-level stmt.
class WithStaticBlock {
  static initialized: boolean;
  static {
    WithStaticBlock.initialized = true;
  }
}
console.log("initialized:", WithStaticBlock.initialized);

// Static block sees the field's declared initializer and overrides it.
class WithFieldAndBlock {
  static value: number = 1;
  static {
    WithFieldAndBlock.value = 42;
  }
}
console.log("value:", WithFieldAndBlock.value);

// Multiple assignments inside a single block.
class MultiAssign {
  static a: number;
  static b: string;
  static {
    MultiAssign.a = 7;
    MultiAssign.b = "ok";
  }
}
console.log("a:", MultiAssign.a, "b:", MultiAssign.b);

// Two static blocks on the same class run in declaration order.
class TwoBlocks {
  static counter: number;
  static {
    TwoBlocks.counter = 0;
  }
  static {
    TwoBlocks.counter = TwoBlocks.counter + 10;
  }
}
console.log("counter:", TwoBlocks.counter);

// The class is visible by name inside its own static block (this is
// the `WithStaticBlock.initialized = true` shape from the issue).
class SelfRef {
  static label: string;
  static {
    SelfRef.label = "self-ref ok";
  }
}
console.log("label:", SelfRef.label);
