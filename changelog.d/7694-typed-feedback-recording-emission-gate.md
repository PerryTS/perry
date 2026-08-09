### Performance

- **Typed-feedback recording calls are no longer emitted into default builds** (#7480 step 4).

  Typed feedback is opt-in. Every one of its recording helpers begins:

  ```rust
  if site_id == 0 || !typed_feedback_enabled() { return; }
  ```

  and `typed_feedback_enabled()` is false unless `PERRY_TYPED_FEEDBACK` /
  `PERRY_TYPED_FEEDBACK_TRACE` is set. Codegen emitted them anyway, on every
  execution of every dynamic property boundary. On `churn_read_big.ts` —
  200k × 1000 reads of `keep[j].v + keep[j].w` — the two that sit on the
  monomorphic-IC hot path are **22.3% of the whole program**:

  | symbol | share of self time |
  |---|--:|
  | `main` (generated code) | 66.0% |
  | `js_dynamic_string_or_number_add` | 11.7% |
  | `typed_feedback::record_guard_pass` | **11.2%** |
  | `typed_feedback::observe_property` | **11.1%** |

  (`sample`, 2465 leaf samples, `PERRY_DEBUG_SYMBOLS=1`.) None of that is
  recording. It is the cross-crate call, plus the `LazyLock<bool>` acquire load
  each helper performs in order to decide it has nothing to do.

  This was never a deliberate trade. The per-site `js_typed_feedback_register_site`
  call has been gated on exactly this env since #5093's follow-up, for exactly
  this reason, and the gate simply stopped short of the recording it registers
  for. `emit_typed_feedback_record_call` now routes all five pure-bookkeeping
  helpers — `observe_property_{get,set}`, `record_guard_{pass,fail}`,
  `record_fallback_call` — through the same switch, at all nine emit sites (the
  generic-get diamond, the array-push fallback, the index realloc arm, the two
  by-name lookup arms, the method-override fallback, the closure-call fallback).

  **The line between gated and not is asserted, not described.** Helpers that
  also perform the operation or pick the dispatch — `js_typed_feedback_*_guard`,
  `…_object_set_field_by_name_fast`, `…_object_get_field_by_name_f64`,
  `…_native_call_method` — are real calls on real paths and are emitted
  unconditionally; a `debug_assert!` on the callee name rejects any attempt to
  route one of them through the eliding helper, and the new test asserts a
  default build still emits `js_typed_feedback_object_set_field_by_name_fast`.

  No behaviour changes in a profiling build (`PERRY_TYPED_FEEDBACK=1` set for the
  `perry` invocation and the run): the existing
  `typed_feedback_instruments_property_and_method_boundaries` test asserts every
  helper is still emitted there, unchanged.

### Fixed

- **A trace requested from a binary that was not built for one now says so.**

  Because registration was already compile-gated, a default-built binary run with
  `PERRY_TYPED_FEEDBACK_TRACE=<path>` could only ever produce *unattributed*
  sites — counters with no module, function or source name. With recording gated
  too it produces nothing at all, which is the same amount of information and
  looks far more like success. `js_typed_feedback_maybe_dump_trace` now prints a
  one-line note when the registry is empty, instead of writing `"total_sites": 0`
  and letting the reader conclude their program has no dynamic boundaries.

  The note names the compile-time env as the *most common* cause rather than
  asserting it, because it is not the only one: an empty registry from a
  correctly-built binary reproduces identically on the pre-change compiler, so
  there is at least one other path that instruments a site and records nothing.
  That is pre-existing and out of scope here; the note is worded so it does not
  send anyone chasing the wrong cause.
