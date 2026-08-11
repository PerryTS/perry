### Fixed

- **The GC trace's `major_pacing` block reports the escalation boundary the collector actually decides on (#7733 review follow-up).**

  `major_pacing_snapshot` recomputed the boundary as `baseline × growth` and dropped the floor on the floor of the function — `let (_floor, growth_num) = major_pacing_config();` — while the predicate it mirrors, `arena_growth_full_escalation_due`, **also** rejects every reading below that same floor. Wherever the floor dominated, the trace named a boundary the collector does not use:

  | state | predicate escalates at | old snapshot reported |
  |---|--:|--:|
  | no full yet (`baseline == 0`) | 32 MB (the floor) | `0` — "escalates at any size" |
  | `baseline = 4 MB`, growth 2 | 32 MB (the floor) | 8 MB |
  | `baseline = 64 MB`, growth 2 | 128 MB + 1 | 128 MB |

  That matters more than the size of the diff because of *why* the snapshot exists. #7733 added it so the pacing subject could be asserted **live** in the trace rather than a gate merely proving nothing threw — which is the one job it could not do while misreporting the quantity. This repo has paid for that shape repeatedly (`PERRY_GC_FORCE_EVACUATE` inert for every `gc()`-driven test, #6942/#6946; the matrix's `--pressure` knob disabling the path it was measuring, #7024; `moved=` summing two different collectors, #7025).

  The fix is structural rather than a second correct formula. There is now **one** definition of the boundary, `major_pacing_escalation_threshold_bytes`: `arena_growth_full_escalation_due_inner` is literally `in_use >= it`, and the snapshot reports it verbatim, floor included. `None` means "no arena reading escalates" — either pacing is disabled (`PERRY_GC_MAJOR_PACING_FLOOR_MB=0`) or the growth term overflowed `usize`, which is the same statement about the world; the helper uses `checked_*` rather than `saturating_*` because saturating would report `usize::MAX` and then claim an arena of `usize::MAX` escalates, which the strict `>` clause never would.

  The trace key follows the semantics: **`escalate_at_or_above_bytes`**, replacing `escalate_above_bytes`. The predicate's floor clause is a `>=`, and the old name was half of why the reported figure and the decision could disagree. `null` now means pacing is off. Nothing in `scripts/`, `.github/` or `docs/` consumed the old key.

- **`zeal_holds_the_poll_word_armed_with_nothing_pending` asserts the release, not just the acquire.**

  The test checked `PERRY_GC_POLL_ARMED > 0` inside the guard scope and then only *narrated* the release in a trailing comment. If `ZealGuard`'s `Drop` ever stopped giving the arm back, the process-global word would stay non-zero for the life of the test binary, every later test would silently take the poll's slow path — and this test would still have passed. It now captures the baseline before the guard and asserts `base + 1` inside and `base` after the drop, the shape `a_deferral_arms_the_poll_word_and_draining_disarms_it` already uses a few lines above.

### Docs

- `docs/src/internals/memory-model.md` no longer contradicts itself about back-edge polls: one line said they became default-on in #7721, and a caveat twenty lines below still said "default off since #7161". The caveat now states the current default and its kill switch, the two gaps that survive it (an alloc-free loop body emits no poll by design, `loop_purity::loop_may_allocate`; the specialized `for` / `for-of` / `for-in` lowerings emit none by omission), and the #7604 exit-70 verdict that makes a vacuous zeal run red instead of green.
- `changelog.d/7729-gc-zeal-allocation-pacing.md` and the matching `gc/zeal.rs` doc drop two overclaims. The `bytes_allocated / stride` bound is qualified to a **positive** stride, since `PERRY_GC_ZEAL_ALLOC_KB=0` is a supported every-poll mode and deliberately outside it. And "reproduces the pre-fix 1:1 behaviour exactly", against a table showing 283,857 collections for 283,852 polls, becomes what the numbers say: one per back-edge poll, plus a handful from the other safepoint zeal forces at — the outermost microtask-pump boundary, which calls `gc_safepoint_moving_minor` without `note_loop_poll_reached`. Near 1:1, not exactly.

### Tests

- `the_reported_escalation_boundary_is_the_one_the_predicate_decides_on` — the named floor-dominates and growth-dominates cases the review called for, plus baseline-zero, the backoff shift and pacing-disabled, then exhaustive over `floor × growth × baseline × shift` probing each boundary's own ±1 neighbourhood. The oracle is a deliberate **independent transcription** of the four clauses the predicate used to spell out inline, not a call into the code under test, so collapsing both onto one helper cannot quietly redefine the rule.
- `the_shipped_predicate_and_the_shipped_snapshot_read_one_boundary` — drives the real `arena_growth_full_escalation_due` against the real `major_pacing_snapshot` on the live arena, so a future re-split fails here even if the pure helper stays correct. `baseline = 0` is the discriminating row and needs no particular heap size: the old snapshot reported `0` there, i.e. `in_use >= 0`, true of every arena including an empty one, while the predicate declines under the floor.
- Both are sabotage-checked against the pre-fix formula, and the `ZealGuard` assertion against a neutered `Drop`.
