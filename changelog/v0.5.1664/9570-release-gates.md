### Fixed

- `Promise.all` now preserves registration order when its input already has `.then()` reactions, while retaining the allocation-free fast path for reaction-free promises.
- The native link-cache regression test now isolates dependency invalidation
  from legitimate cross-module specialization, restoring its cache-hit
  coverage in the full release gate.
