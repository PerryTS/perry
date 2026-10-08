Byte access proofs are now installed only by the existing parameter and loop
pre-passes, before calls are lowered. Write-only receivers and buffer intrinsics
without a registered proof resolve their headers at each access, preserving
detach and transfer behavior after callbacks. SharedArrayBuffer and NativeArena
owners retain their runtime atomic and disposal rules. Regression tests cover
these paths, mutable module bindings, namespace writes and detached lengths,
with negative controls for late installation and unsafe owner admission.

Receiver invalidation consumes a typed memory-effect accessor from the same
helper contract used for LLVM declarations. Audited noncollecting read-only
calls preserve both byte-view and stable-packed proofs. While loops enter the
existing Number tier, including loops outside byte scanning; full-program
measurements check that behavior.

Dynamic numeric reads of NativeArena views now use the validated typed-array accessor before bounds, so disposal still throws after its length becomes zero. Transfer and disposal witnesses use runtime loop bounds to avoid HIR unrolling.
Loop pre-registration also covers indexed writes and declarations inside loops, before initializer callbacks.
