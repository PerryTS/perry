# Lazy canonical R3 focused replay

Nine repetitions per case and three randomized Perry arms: lazy canonical R3, freshly built merged main eee3881c4, and tape-depth R2. Node is the output oracle. All 20 output checks and 135 timing trials passed, under the archived quiet-host window. CPU is process CPU per call, with all samples retained. Differences are descriptive; overlapping ranges do not prove equality.

| Fixture | Operation | Candidate us | Main us | PR build us | vs main | vs PR build | Slower pairs | Peak delta KiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| records_array_16k | roundtrip | 33.326400 | 24.863200 | 20.539650 | +34.039% | +62.254% | 9/9 | +176 |
| records_array_1m | roundtrip | 2200.666016 | 1681.382812 | 1395.517578 | +30.884% | +57.695% | 9/9 | +144 |
| records_array_8m | roundtrip | 17970.984375 | 14351.390625 | 12298.421875 | +25.221% | +46.124% | 9/9 | +160 |
| heterogeneous_1m | stringify | 677.655273 | 675.085938 | 678.323242 | +0.381% | -0.098% | 8/9 | +144 |
| small_record | parse | 0.096389 | 0.097157 | 0.096639 | -0.790% | -0.258% | 0/9 | +160 |

R3 is rejected for landing: record roundtrips remain 25–34% slower than main
and 46–62% slower than the current PR build, with 9/9 slower pairs at each size.
The preceding R2 window measured 28–36% slower than main; those are separate
windows, not a randomized R2-versus-R3 comparison. No full matrices are warranted.
All three benchmark worker hashes match the archived source/build provenance.
The full before/after process listings are retained with the qualified window.
