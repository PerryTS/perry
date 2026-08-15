`perry compile` no longer SIGBUSes on production Next.js bundles that contain a preserved loop.

Perry emits `call void asm sideeffect "", ""()` as the issue-#74 loop-preservation barrier. `rewrite-statepoints-for-gc` rewrites every non-leaf call in a `gc "statepoint"` function into a `gc.statepoint`, and for inline asm that means using the `InlineAsm` itself as the statepoint's callee operand — invalid IR, rejected by the verifier with `Cannot take the address of an inline asm!`.

`optimize_and_emit` verified the module *before* the rewrite but not after, so the broken module went straight to SelectionDAG, which dereferenced the bogus callee and killed the compiler with `SIGBUS` inside `AArch64TargetLowering::LowerCall`. Compiling next@16.3.0's bundled `jsonwebtoken` died this way 100 modules into a 104-module production App Route, blocking #8040.

The barrier now carries `"gc-leaf-function"` on both the native and text emission paths, which is the documented way to tell RS4GC a call cannot trigger a collection — true by construction for a barrier that emits no instructions. `optimize_and_emit` additionally verifies *after* RS4GC, so this class of bug is a clean compile error naming the pass instead of a crash that takes the compiler process down.

Fixes #8121. Refs #8040.
