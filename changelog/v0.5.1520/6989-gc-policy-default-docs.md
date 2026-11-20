Corrected two GC policy-gate doc comments that claimed the opposite of the code's
actual default. `gc_incremental_enabled` said "EXPERIMENTAL — default OFF" while
defaulting ON since the #6180 flip, and `gc_safepoint_moving_minor` described its
gate as "(default off)" when `gc_moving_safepoint_enabled` is also default ON.

This was not cosmetic: the stale comment led #6972 to dismiss the production
reachability of an unrooted-operand bug (#6970) on the reasoning that the
automatic collection arms force a conservative stack scan. With the incremental
stepper on, budgeted cycles skip that subphase structurally, so a compiled
program completes precise-roots-only cycles in its shipped configuration —
measured at 2 of 3 cycles with `conservative_root_count = 0` and no environment
variables set. The corrected comment now records why the default is load-bearing
for rooting arguments, not only for pause times.
