Reduce ordinary closure call overhead by reusing the validated body record
and dispatching its supplied argument slice directly, without a second
closure lookup or a padded scratch buffer. Keep missing-argument padding,
rest/arguments bundling, receivers, proxies, bound calls and legacy INT32
conversion at their existing semantic boundaries.

The same record now proves real non-constructor builtin bodies callable
without native-export or constructor probes. Their `call` forwarding shares
ordinary body entry; no-op prototype placeholders retain their name forwarder,
and native `apply` retains the rooted array-like getter path. Small declared
signatures load their arguments directly instead of filling a six-slot buffer.

Borrowed `Array.prototype.push`, `unshift` and `splice` native argument lists
now keep mutable native cells rooted while the shared generic mutator engine
can run setters or traps. Preserve the dense push paths and native-list ABI
already introduced by the builtin receiver lane; moving GC rewrites the cells
that the engine reads.
