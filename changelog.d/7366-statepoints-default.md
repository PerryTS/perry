### Changed

- **Native GC roots (statepoints) are now the default.** `PERRY_RS4GC=1` is no
  longer needed; `PERRY_RS4GC=0` reverts to the shadow stack for bisection.

  The default is **target-aware**, not blanket: native roots where the runtime
  can walk the frames, shadow stack where it cannot. `gc_map` deliberately
  *refuses* to emit a map for a target whose frame bases the runtime cannot
  resolve — a map nothing reads loses roots silently — so a global flip would
  turn every watchOS `arm64_32` and ARM64-Windows compile into a hard error.
  Falling back is not "no roots"; it is the other lowering of the same
  root-set analysis, which #7340 split apart precisely so this choice could be
  made per target.

  An explicit `PERRY_RS4GC=1` still reaches that refusal rather than being
  quietly downgraded, so an A/B arm measures what it asked for.

  Evidence: full 479-test gap suite with no env set — **447 pass / 19 diff /
  13 node_fail, identical to the shadow-stack baseline**, zero new regressions
  and zero compile failures, including all 128 try-carrying tests. All 10
  `gc_ratchet` probes byte-identical to Node. Runtime −1–2%, binary size +1.86%
  on a real dependency (zod, 81 modules).

  Eight codegen tests that assert on shadow-stack IR now pin that lowering
  explicitly via a thread-local test guard. They were correct about what they
  asserted; they had simply never had to name a lowering, because there was
  only one default.
