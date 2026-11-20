fix(wpfacts): per-unit facts are the default facts mode

Since the closed-world solver landed, the driver ran the whole-program solve by
default. Whole-program exact points-to does not scale: fastify exceeds the 200M
work bound (aborted, no facts) and does not converge even unbounded. Per-unit
facts solve fastify with 12.8M work and prove 69 sites.

Per-unit is now the default (owner decision 2026-10-23). `PERRY_FACTS_UNITS=0`
selects the whole-program solve, kept as the reference mode.
