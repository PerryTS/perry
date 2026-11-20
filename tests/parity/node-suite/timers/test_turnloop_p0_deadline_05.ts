// turnloop P0 loop-statistics probe (scripts/turnloop_p0_loop_stats.py): one
// 0.5 ms timeout. Perry treats a sub-millisecond delay as due at once, so this
// may legitimately need no OS wait at all; it must never spin.
setTimeout(() => console.log("deadline 0.5 hit"), 0.5);
