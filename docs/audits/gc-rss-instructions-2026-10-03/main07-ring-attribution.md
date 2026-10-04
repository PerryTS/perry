# Natural ring churn: nursery occupancy attribution

On current07, the unchanged 256-record ring with two million replacements had 29 natural copying minors. All three policies (production GC, repeated-entry advice, once-per-idle-interval advice) had the same cycle count and heap geometry. Median Eden survivors were 12,288 bytes; maximum 14,528. Steady pre-minor nursery occupancy was 3,145,728 bytes; median nursery capacity was 4,194,304 bytes. Survivor spaces retained their 1 MiB targets. This makes the learned low-survival nursery floor a concrete experiment, rather than an unconditional collection-frequency change.

These are full-runtime diagnostic builds, one plain and one trace execution per arm, not normal-auto acceptance. Trace RSS is perturbed by telemetry; mapped capacity is not necessarily resident. Plain production peak RSS was 17,862,656 bytes; the independent normal-auto V40 ring result remains 14.39 MiB. These observations do not establish an absolute RSS floor.

The new private learned-floor worktree starts from deferred-RSS-query commit d4f491673b. It retains quarter-base startup sizing and the existing two-cycle 1% shrink / 4% growth rules, adds lower ladder levels to a sixteenth-base floor, and keeps configured-base survivor/lifetime budgets unchanged. Runtime and application validation are pending; no adoption or benefit claim.
