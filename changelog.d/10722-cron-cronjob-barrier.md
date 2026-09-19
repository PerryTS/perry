**`test_gap_cron_cronjob` no longer races the wall clock inside `pr-gate`'s
scope.** The fixture started a `* * * * * *` CronJob, then waited with
`while ((ticks < 2 || autoTicks < 2) && Date.now() < tickDeadline)` against a fixed
`Date.now() + 10_000`. That deadline is a *timeout*, not a barrier: when it expired
first the loop exited and the fixture printed `false` on two lines that are expected
to read `true`, and dropped the `tick 1` / `tick 2` lines entirely — a four-line
output divergence the harness classifies as a `parity_fail`, i.e. reports as a
compiler regression. The header comment's claim that the output was "deterministic
despite the timing" held only while two ticks of a one-per-second schedule landed
inside ten seconds. It cost real work: the fixture failed in a merge-queue
validation and #10530 was held out of a train on the strength of it, after which a
`--trace llvm` A/B showed byte-identical IR.

The wait is now a **barrier with no deadline**. The printed text becomes a function
of CronJob's behaviour alone: either the ticks arrive and the fixture prints its one
expected output, or nothing dispatches and the harness's own `PERRY_RUN_TIMEOUT`
kills the run — which it classifies as a CRASH/timeout, distinctly from a parity
mismatch, so a contended runner can no longer make this look like a miscompile.
There is deliberately no fallback bound: any bound that prints, throws or exits
differently on expiry reintroduces the same defect at a different threshold, and a
`false`-printing 30s deadline is the identical bug with a longer fuse. The old
number was in any case unreachable — `PERRY_RUN_TIMEOUT` is itself 10s, so the
fixture's deadline could only ever fire in a photo finish with the kill.

Nothing is weakened. The assertion moved from a printed comparison into the loop's
exit condition, which the program cannot pass without satisfying; the four-arg
`start=true` form, the non-auto-starting two-arg form and `start()`/`stop()`
dispatch are all still exercised, and the `tick 1` / `tick 2` lines remain in the
diff as positive evidence that the manual job fired. The never-started job's line
got *stronger*: it printed a hardcoded `true`, and now prints `neverTicks === 0`
from a real counter, checked after the barrier — i.e. after at least two cron
seconds have demonstrably elapsed with that job unstarted. Output bytes are
unchanged.

Verified by injecting an identical 11-second synchronous event-loop stall into the
old and new fixtures at the same point (a deterministic stand-in for the loaded
runner). Under Node 26.5.1 and under Perry v0.5.1598 alike, the old fixture prints
the four-line divergence and the new one is byte-identical to the unstalled oracle.
The real fixture passes the harness (`run_parity_tests.sh --filter
test_gap_cron_cronjob`, exit 0, journal `status: pass`), and eight consecutive Node
runs gave one distinct output in 1.86–2.04 s — roughly a fifth of the run budget.

Two sibling fixtures have the same shape and are *not* touched here:
`test_gap_9592_child_timeout_threads` (a 1 s deadline whose expiry prints
`timeout threads released: false`; Linux-only, short-circuited elsewhere) and
`test_gap_9493_child_stdin_backpressure` (a watchdog that `resolve(false)`s). Both
are in gate scope. (#10581)
