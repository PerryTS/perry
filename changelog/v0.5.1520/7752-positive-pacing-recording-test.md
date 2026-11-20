### Changed

- **gc: the arena-growth pacing recording is now asserted in the POSITIVE direction, and the predicate and the recording read one accessor (#7737 item 2).**

  `declining_to_escalate_records_no_pre_full_reading` proved only that a *declined* escalation leaves `pre_in_use == 0`. Nothing asserted that a `true` verdict from the real `arena_growth_full_escalation_due()` — not the `test_note_full_cycle_reclaimed` bypass, which sets the reading itself — leaves a non-zero one behind. A negative-only test cannot distinguish "declined correctly" from "the recording never ran": both produce `0`. That is precisely the first-cut bug #7733's changelog describes (the recording wired at the wrong call sites, `update_major_pacing_backoff` returning early on a zero pre-reading, every test green).

  Forcing `due == true` needs an arena reading above `PERRY_GC_MAJOR_PACING_FLOOR_MB` (32 MB by default), and the floor cannot be lowered per test: `major_pacing_config` is a process-wide `OnceLock`, so an env var only takes effect if the test happens to run first. A 32 MB live heap in a unit test is what `major_pacing_escalation_threshold_for` was factored out to avoid. So the reading is injected instead, through a new `pacing_arena_in_use_bytes()` — the ONE accessor both the escalation predicate and `note_full_cycle_started` now use.

  That accessor is worth more than the seam. `update_major_pacing_backoff`'s doc already claimed the two are "deliberately measured on the SAME metric ... so the two cannot disagree about whether a full helped" — but both sites called `arena_in_use_bytes()` independently, so the guarantee was a convention, not a structure. Routing an injected reading in and requiring the same value back out makes it checkable. The override is `#[cfg(test)]`, so it compiles out of every shipping build; it is not a runtime knob and so is not subject to the GC knob kill-policy.

  Three tests: the positive recording, predicate/recording agreement across several readings, and the floor's inclusivity (`>=` on the floor is why `major_pacing_escalation_threshold_for` adds its `+1` to the growth boundary and not to the floor).

  **All three were verified able to fail.** Sabotage 1 rewires the recording away from the predicate (the #7726 bug): all 3 go red. Sabotage 2 points `note_full_cycle_started` back at a second, independent `arena_in_use_bytes()` call (metric drift): all 3 go red. In both arms the pre-existing negative test still PASSED — which is the gap this closes, demonstrated rather than asserted.

  Items 1 (prototype-registry latch) and 4 (branch-protection promotion) of #7737 are not touched here: 1 landed in #7740, and 4 is a maintainer action. Item 3 (the grow-then-churn `gc_ratchet` probe) needs the pinned bench host and is left open.
