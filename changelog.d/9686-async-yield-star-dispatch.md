**Async generators with many `yield*` sites no longer make the compiler appear
to hang while consuming tens of gigabytes.** The `.throw()` lowering cloned the
entire state dispatcher into every delegation route, making transformed HIR
quadratic in the number of delegation sites and multiplying LLVM safepoints and
relocations. Delegation routing now selects one mutually exclusive branch and
falls through to a single shared dispatcher. On the real-world 32-site function
that exposed the bug, the generated async-step closure shrank from 33.6 MB to
2.5 MB of debug-rendered HIR (92.5%).

The fix preserves `.throw()` behavior across multiple delegated generators and
still routes delegation-protocol failures through the outer generator's
`try`/`catch`; both paths are covered by a Node-parity fixture. A structural
transform test also rejects future super-linear dispatcher growth.
