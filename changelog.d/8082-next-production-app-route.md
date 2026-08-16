### Fixed

- Preserve production Next.js App Route request state through generated
  `AppRouteRouteModule.handle` dispatch, imported handlers, async
  continuations, and separately loaded runtime/stdlib providers (#8036).

- Keep native statepoint roots in an app dylib. The demotion of
  `--output-type dylib` artifacts to the shared shadow stack predated
  #8081's loaded-image stack-map indexing; with that in place it would
  leave provider apps running a lowering production never ships, and it
  defeated #8081's own gate assertion that the app's map survives macOS
  dead stripping.

- Class-self lowering respects a same-named method parameter or local
  instead of forcing the lexical class binding; computed `require(".")` /
  `require("..")` resolve relative to the caller; dynamic virtual dispatch
  builds its direct-call ABI from the selected override's own metadata,
  including rest and synthetic `arguments` shape in both override
  directions; bound-method construction roots the receiver across closure
  allocation and the closure across allocating metadata installation;
  malformed unwind-table records are parsed transactionally with checked
  ranges and offsets.

### Added

- A pinned Next 16.3.0 production App Route fixture
  (`tests/release/packages/next-app-route/`): the untouched webpack output
  is compiled as an app-only dylib against separate runtime and stdlib
  provider images, then served through a `dlopen` host and compared with
  the Node production oracle over 10 cold starts, each running two
  21-request verifier passes.

  A forced-evacuation arm is available behind
  `PERRY_NEXT_ROUTE_FORCED_GC=1` and is **not** part of the default gate:
  it currently fails (#8163). When enabled, alternate cold starts run under
  forced evacuation with GC diagnostics and their moving-GC liveness is
  asserted by `scripts/gc_evacuation_liveness_assert.py`, so zero copying
  minors or zero copied objects is a hard failure rather than a vacuous
  pass. It is deliberately neither a `SKIP` (which would read as covered)
  nor `continue-on-error` (which would make it documentation rather than a
  gate): off by default, failing loudly when set.

- `PERRY_GC_PROTECT_FROMSPACE_HOLDERS=1`: at a from-space fault, sweep the
  whole live heap for any word that still decodes to the faulting address
  and name the owners. The existing report answers "who used it" — the
  consumer, which for a value read out of a table one instruction earlier
  is never the bug; this answers "who kept it".
