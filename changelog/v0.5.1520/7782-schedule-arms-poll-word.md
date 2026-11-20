### Fixed

- **The seeded GC schedule never armed the poll word, so `PERRY_GC_SCHEDULE_RATE=1` was an event-loop-boundary instrument only (#7781).** On #7606's reproduction it saw **6 safepoints** where zeal's runs cross **9,648 / 19,248 loop polls** — the "collect at every opportunity" end of the dial saw six opportunities. #7735 collapsed the back-edge poll's no-work path to one load of `PERRY_GC_POLL_ARMED`; `resolve_poll_seed` kept the word armed for zeal, and nothing armed it for the schedule mode, so the loop-safepoint bypass #7317 added sat behind a gate that never opened.

  `resolve_poll_seed` now keeps the startup seed when `schedule::gc_schedule_enabled()`, exactly as for zeal, and `ScheduleGuard` mirrors `ZealGuard`'s arm/release pair so test-time schedules reach polls too. Regression test `the_schedule_holds_the_poll_word_armed_like_zeal` mirrors the zeal test and is sabotage-verified (removing the arm fails it).

  Re-run with the fix, same binary, same quarantine (`PERRY_GC_PROTECT_FROMSPACE=1`, depth 800):

  | | before | after |
  |---|--:|--:|
  | `rest_argument_rooting` | safepoints=6 | **safepoints=9,653**, 9,653 collections, exit 0 |
  | `same_module_call_rooting` | safepoints=6 | **safepoints=19,253**, 19,253 collections, exit 0 |

  This was found testing #7741's precondition — that `SCHEDULE_RATE=1` be demonstrated equivalent to zeal on a real reproduction before the older instrument is deleted. The precondition now holds; without this fix it measurably did not.
