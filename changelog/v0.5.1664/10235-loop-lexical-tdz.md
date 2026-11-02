Captured forward `let` and `const` bindings now receive fresh TDZ cells at their
own block entry. Repeated loop entries throw `ReferenceError` before each
declaration, uninitialized `let` declarations end that entry's TDZ with
`undefined`, and retained callbacks keep their original iteration's binding.
Function-scoped `var` bindings continue to share one cell.

Switch cases allocate one shared lexical environment after the discriminant,
and TDZ cells precede hoisted block-function closures. Code generation also
allocates a fresh cell in every emitted copy of a `finally` block, preserving
the shared stack slot across its normal and exceptional paths.

Adds HIR and LLVM regressions plus a bounded byte-for-byte Node/native suite
covering script and module contexts at O0/Os/Oz with default and compact GC
configurations, retained callbacks, recursion, skipped declarations, and
exceptional `finally` paths.
