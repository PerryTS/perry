Initialize the current thread's GC root scanners before independently bootstrapping
the Object intrinsics or their shape-only prototype sentinel. Property misses can
reach these paths before `globalThis` exists, including from native callers and
worker threads. Without initialization, runtime handles are neither traced nor
rewritten: a getter's moving collection prunes the live transform pair's shape,
so its next endpoint lookup returns `undefined` and `pipeThrough` throws
`TypeError: Invalid transform writable`.

The failure became visible after `64eca2c2ca` replaced the property-miss path's
realm-global bootstrap with the independent Object-intrinsic bootstrap. Keep
`streams::tests::pipe_through_pair_survives_a_moving_getter` as the regression,
and require each endpoint getter to run exactly once and move the pair itself.
