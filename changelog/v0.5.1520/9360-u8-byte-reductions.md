**Typed-array hot loops now retain their native numeric reductions.**
Buffer-backed `Uint8Array` reads recover the correct element on typed-array
registry misses and use a guard-validated inline byte-load lane for
module-global and declared-parameter receivers. Construction-proven
module-global numeric views now feed the same Number-by-construction proof as
body-local views, removing the rooted dynamic-add diamond from byte sums.

Bounded byte reductions may carry per-instruction `reassoc` when their complete
integer magnitude proof stays within the exact f64 range, allowing LLVM to
split the serial accumulator without enabling unsound global fast-math.
Module-init accumulators proven never to hold pointers also shed redundant
shadow slots and back-edge GC polls. On the measured `bench_buffer_readwrite`
shape this takes Perry from 94 ms to 34 ms against Node's 81 ms, while
unbounded f64 reductions remain unchanged.
