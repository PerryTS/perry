The copying minor's adaptive tenuring threshold no longer climbs to 4 by
occupancy. When few objects survived a minor, the occupancy rule raised the
promotion age to 4, so every survivor was copied up to three times before it
could be promoted. The rule is now capped at 3. The survival-rate lock (promote
on first copy when a fresh cohort comes back at least 90 % alive) and the sweep
seed are unchanged and still reach 1. `PERRY_GC_TENURING_SURVIVALS_MAX=4`
restores the previous climb, and `PERRY_GC_TENURING_SURVIVALS=<n>` still pins
the age for every minor. Measured against the previous climb: chess search
-0.77 % instructions, a medium-lived record benchmark -0.57 to -0.77 %, other
programs unchanged, peak RSS unchanged.
