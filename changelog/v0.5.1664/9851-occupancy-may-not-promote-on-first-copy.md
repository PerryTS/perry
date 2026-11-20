### Fixed

- **The adaptive tenuring loop's occupancy rule can no longer conclude
  "promote on first copy" — a claim about lifetime that it has no evidence
  for, and which destroys the evidence that would refute it.**

  `retune_after_scavenge` picks a survival threshold from
  `S = 1 + desired / influx`: the largest S whose projected survivor occupancy
  `(S-1) x influx` fits the desired survivor size. With integer division, any
  influx above `desired` yields exactly **1** — there is no rung at 2 or 3.
  Measured on the compiled claude-code TUI, the first drop reads
  `eden_live_bytes=12075344` against `desired=1048576`.

  S=1 does not reduce the surviving data; it relocates it, from the survivor
  space — where the next minor re-examines it for free — to the old generation,
  which only a full collection can reclaim. The occupancy formula has no term
  for that. And S=1 is **self-sealing**: nothing is copied, so `copied_bytes` is
  0, so next cycle `prev_copied` is 0, so the survival-rate lock's guard
  (`prev_copied >= substantial`) is false forever. Both remaining exits — the
  occupancy recompute and `PROMOTE_LOCK`'s unlock — are *quiet-influx* exits,
  which say nothing about lifetime.

  Measured, 4 streamed turns in one process, both arms from one binary via the
  diagnostic knob `PERRY_GC_TENURING_SURVIVALS`, 3300-character replies:

  | | adaptive | pinned S=2 |
  |---|---|---|
  | minors at S=1 | **351 of 352**, carrying 100 % of promotion | 0 |
  | threshold transitions in the whole run | **1** | 7 |
  | survivor-round mortality samples | **1** | **393** |
  | median mortality | **0.9 %** | **26.1 %** |
  | ...in steady turns 2 / 3 / 4 | not measurable | 26.1 / 26.1 / 26.1 % |
  | promoted | 1057 MB | 792 MB |

  The loop takes its one and only mortality measurement on the **first minor of
  the process** — before any steady state, when the cohort really is immortal —
  reads 99.1 % survival, drops to 1, and can never sample again. In steady
  state an aging round filters about **a quarter** of each cohort.

  The occupancy rule now stops at the lowest threshold that still *produces*
  that measurement. That value is 2 by construction, not by tuning: at S=1
  nothing enters the survivor space, at S=2 exactly one cohort does. The
  arithmetic is untouched — `compute_target_survivals` still computes 1, and
  its test asserts so byte-identically; only what the loop may do with the
  result changes.

  **Reaching 1 still belongs to the two paths that measure mortality** — the
  survival-rate lock (a substantial cohort of which >= 90 % came back alive)
  and the sweep seed (the mark-sweep's own Eden live/dead split). Both are
  untouched, so the rule is self-limiting: on a workload whose cohort genuinely
  does not die, the lock fires after one cohort's copy and takes the loop back
  to 1.

  On claude-code it **does** fire, 8-12 times per four-turn run, and the
  companion entry below is why: once the clamp lets the ladder climb past 2 the
  lock is rating a population its own threshold selected. An earlier version of
  this entry claimed the opposite ("5 of 358 substantial cohorts sit under the
  lock's threshold, so the clamp holds rather than oscillating"); that figure
  was measured with the threshold *pinned*, where every cohort the lock can
  rate is a first-round cohort, and it does not describe the rule running.

  Two existing tests change their expected value from 1 to 2 and keep their
  names, structure and invariants: `drops_immediately_and_rises_debounced`
  protects the *asymmetric response* (immediate drop, debounced rise), which
  4 -> 2 demonstrates exactly as well as 4 -> 1; and
  `steady_heavy_influx_is_a_fixed_point` protects *fixed-pointness*, which is
  unchanged with 2 as the fixed point.
