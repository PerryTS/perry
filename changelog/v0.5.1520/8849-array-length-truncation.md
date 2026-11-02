Strict writes to a statically proven Array's `length` now retain their ArraySetLength lowering,
and ordinary dense truncation clears discarded slots in one guarded runtime region instead of
performing descriptor and side-table deletion work for every element. The generic path remains for
sloppy, explicit-receiver, sparse, and descriptor-bearing cases. On the unchanged codehz/ecs
15k-command workload, an 11-pair Apple-silicon cohort improved the median from 31.541 ms to
28.354 ms (10.03%, 11/11 wins, 22/22 semantic-oracle passes).
