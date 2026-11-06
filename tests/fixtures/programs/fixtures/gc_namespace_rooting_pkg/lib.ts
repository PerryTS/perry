// Companion module for test_gap_gc_namespace_and_computed_dispatch_rooting.ts
// (#7210 section 2). `import * as ns from "./lib"` in the entry module
// forces `codegen/helpers.rs`'s `emit_namespace_populator` to run at this
// module's init, staging every export below (a plain var, a function, a
// class, and a re-exported var) into the `vals_buf` alloca the fix roots.

export const tag = "lib";

export function double(x: number): number {
  return x * 2;
}

export class Box {
  value: number = 0;
  constructor(v: number) {
    this.value = v;
  }
}

// A re-export ("ForeignVar"/"ForeignFunction" NamespaceEntryKind), which
// routes through a cross-module getter call in the populator loop.
export { churnFromOther, CHURN_TAG } from "./other.ts";
