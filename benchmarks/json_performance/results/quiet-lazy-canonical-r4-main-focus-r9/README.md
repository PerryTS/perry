# Lazy canonical R4 focused replay

Nine repetitions per case and three randomized Perry arms: lazy canonical R4, freshly built merged main eee3881c4, and tape-depth R2. Node is the output oracle. All 32 output checks and 216 timing trials passed, under the archived quiet-host window. CPU is process CPU per call, with all samples retained. Differences are descriptive; overlapping ranges do not prove equality.

| Fixture | Operation | Candidate us | Main us | PR build us | vs main | vs PR build | Slower pairs | Peak delta KiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| records_array_16k | roundtrip | 30.201700 | 24.849300 | 20.554150 | +21.539% | +46.937% | 9/9 | +208 |
| records_array_1m | roundtrip | 1993.267578 | 1682.583984 | 1395.861328 | +18.465% | +42.798% | 9/9 | +176 |
| records_array_8m | roundtrip | 16445.640625 | 14345.453125 | 12239.531250 | +14.640% | +34.365% | 9/9 | +192 |
| heterogeneous_1m | stringify | 675.146484 | 674.229492 | 678.304688 | +0.136% | -0.466% | 8/9 | +208 |
| small_record | parse | 0.096435 | 0.097000 | 0.096538 | -0.582% | -0.107% | 1/9 | +224 |
| records_array_1m | parse | 924.882812 | 1190.191406 | 909.541016 | -22.291% | +1.687% | 0/9 | +144 |
| records_array_1m | scan | 3055.253906 | 3325.535156 | 3030.457031 | -8.127% | +0.818% | 0/9 | +208 |
| records_array_1m | sparse | 983.474609 | 1249.527344 | 969.591797 | -21.292% | +1.432% | 0/9 | +176 |

R4 is rejected for landing: record roundtrips remain 15–22% slower than main
and 34–47% slower than the current PR build, with 9/9 slower pairs at each size.
The added parse-cost controls measure 1 MiB parse +1.687%, scan +0.818% and sparse
+1.432% versus the PR build. They remain faster than main because they retain
the earlier tape-depth improvement; that does not erase the incremental cost.
Peak RSS deltas versus main span +144 to +224 KiB across these eight cases.
No full matrices or broader absence-of-regression claim are warranted.

All three worker hashes match their archived source/build provenance. The full
before/after process listings and exact runners are retained with the qualified
07:58:48–08:01:16 UTC window on 2026-09-10. Prior R3 timings come from a separate
window; this run is randomized against main and tape-depth R2, not R3.
