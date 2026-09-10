# Lazy canonical R5 focused replay

Nine repetitions per case and four randomized Perry arms: lazy canonical R5, freshly built merged main eee3881c4, tape-depth R2, and preceding lazy canonical R4. Node is the output oracle. All 40 output checks and 288 timing trials passed, under the archived quiet-host window. CPU is process CPU per call, with all samples retained. Differences are descriptive; overlapping ranges do not prove equality.

| Fixture | Operation | Candidate us | Main us | PR build us | vs main | vs PR build | vs R4 | Slower pairs/main | Slower pairs/R4 | Peak delta KiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| records_array_16k | roundtrip | 30.021750 | 24.836950 | 20.551050 | +20.875% | +46.084% | -0.377% | 9/9 | 0/9 | +176 |
| records_array_1m | roundtrip | 1996.421875 | 1683.634766 | 1396.427734 | +18.578% | +42.966% | +0.074% | 9/9 | 8/9 | +160 |
| records_array_8m | roundtrip | 16455.281250 | 14347.484375 | 12241.125000 | +14.691% | +34.426% | -0.080% | 9/9 | 2/9 | +176 |
| heterogeneous_1m | stringify | 675.125977 | 674.828125 | 677.096680 | +0.044% | -0.291% | +0.066% | 6/9 | 5/9 | +192 |
| small_record | parse | 0.096479 | 0.097167 | 0.096739 | -0.708% | -0.269% | -0.100% | 2/9 | 3/9 | +192 |
| records_array_1m | parse | 924.613281 | 1191.205078 | 910.031250 | -22.380% | +1.602% | -0.014% | 0/9 | 6/9 | +128 |
| records_array_1m | scan | 3062.945312 | 3331.710938 | 3033.390625 | -8.067% | +0.974% | +0.079% | 0/9 | 6/9 | +176 |
| records_array_1m | sparse | 983.882812 | 1247.927734 | 970.375000 | -21.159% | +1.392% | +0.148% | 0/9 | 4/9 | +160 |

R5 is rejected for landing. Its record roundtrip changes versus the preceding
R4 are -0.377%, +0.074% and -0.080%, with 0/9, 8/9 and 2/9 slower pairs. It does
not resolve the 15–21% slowdown versus main or 34–46% versus the PR build.
The 1 MiB parse control remains +1.602% versus the PR build. All medians and
samples are retained; no full matrices or general no-regression claim are made.
Peak RSS deltas versus R4 are recorded separately in reference-screen.json.

All four worker hashes match their archived source/build provenance. The exact
four-arm runner and full before/after process listings are retained with the
qualified 08:27:15–08:30:34 UTC window on 2026-09-10. The prior source remains
parked alongside this one; neither canonical correction is in the PR runtime.
