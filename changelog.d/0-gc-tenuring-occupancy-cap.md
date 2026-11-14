The copying minor's adaptive tenuring threshold no longer climbs above 2 by
occupancy. When few objects survived a minor, the occupancy rule raised the
promotion age from 2 to 3 and then 4, so every survivor was copied up to three
times before promotion. The rule is now capped at 2. The survival-rate lock
(promote on first copy when a fresh cohort comes back at least 90 % alive) and
the sweep seed are unchanged and still reach 1.
`PERRY_GC_TENURING_SURVIVALS_MAX=4` restores the previous climb, and
`PERRY_GC_TENURING_SURVIVALS=<n>` still pins the age for every minor.
