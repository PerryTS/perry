### Fixed

- **The tenuring survival-rate lock now rates one fresh cohort, not the whole
  survivor space — a well-formed ratio that stopped describing what it is named
  after as soon as the threshold it sets rose above 2.**

  The lock exists to answer "did an aging round filter anything?" and, when the
  answer is no, to promote on first copy. It tested

  ```
  prev_copied >= substantial && survivor_live_bytes * 10 >= prev_copied * 9
  ```

  where `survivor_live_bytes` is every live byte leaving the from-survivor space
  this cycle, of any age, and `prev_copied` is the previous cycle's whole intake
  into that space. Those two scopes match — the survivor spaces are a strict
  semispace pair, so the from-space holds exactly what the last cycle copied —
  and the ratio cannot exceed 1. **The defect is not the arithmetic; it is which
  population the ratio rates, and that is chosen by the very threshold the lock
  sets.** At a threshold of 2 the space holds one fresh cohort and the ratio is
  one aging round's survival. At 3 or 4 it also holds objects that have already
  survived a round and are therefore selected for longevity, so the aggregate
  clears the 90 % bar while a fresh cohort does not. The rule reads its own
  setting back as evidence.

  This was invisible while the occupancy rule sealed the loop at S=1, because
  there `copied_bytes` is 0 and the lock's guard can never be satisfied.
  Removing that seal handed the lock its guard back, and it became the dominant
  route to promote-on-first-copy.

  Measured on the compiled claude-code TUI, one binary, three arms via
  `PERRY_GC_TENURING_SURVIVALS`, 3300-character replies, 4 turns in one process:

  | arm | minors | promoted | S=1 share of promotion | reached 1 via the lock |
  |---|---|---|---|---|
  | `=1` (pre-clamp equivalent) | 356 | 1055 MB | 100 % | - |
  | occupancy clamp only | 368 / 384 | 982 / 980 MB | 85 % / 84 % | **8 / 12** |
  | `=2` (positive control) | 380 | 785 MB | 0 % | n/a |

  The copier now also accounts the fresh half of each cycle: `eden_copied_bytes`
  (bytes copied out of *Eden* into the to-survivor space, no re-copies) and
  `survivor_first_round_live_bytes` (live bytes leaving the from-survivor space
  whose stored survival age is 1, i.e. members of exactly the cohort the
  previous cycle's `eden_copied_bytes` counted). The lock rates those two. Both
  are on the `[gc-copy-minor]` diagnostic line, so first-round mortality is
  readable from any build rather than only from an instrumented one.

  Reaching 1 still belongs to the paths that measure mortality; what changes is
  that the measurement is now of one aging round at every threshold.
