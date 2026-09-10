# Lazy canonical R2 focused replay

Nine repetitions per case and three randomized Perry arms: lazy canonical R2, freshly built merged main eee3881c4, and tape-depth R2. Node is the output oracle. All 20 output checks and 135 timing trials passed, under the archived quiet-host window. CPU is process CPU per call, with all samples retained. Differences are descriptive; overlapping ranges do not prove equality.

| Fixture | Operation | Candidate us | Main us | PR build us | vs main | vs PR build | Slower pairs | Peak delta KiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| records_array_16k | roundtrip | 33.819700 | 24.837950 | 20.544950 | +36.161% | +64.613% | 9/9 | +624 |
| records_array_1m | roundtrip | 2226.898438 | 1681.406250 | 1394.863281 | +32.443% | +59.650% | 9/9 | +112 |
| records_array_8m | roundtrip | 18313.906250 | 14332.218750 | 12246.812500 | +27.781% | +49.540% | 9/9 | +128 |
| heterogeneous_1m | stringify | 677.867188 | 674.936523 | 678.288086 | +0.434% | -0.062% | 9/9 | +144 |
| small_record | parse | 0.096409 | 0.096993 | 0.096435 | -0.602% | -0.027% | 0/9 | +192 |

R2 is rejected on CPU cost. Record roundtrips are 27.8–36.2% slower than main
and 49.5–64.6% slower than the current PR build, with 9/9 main pairs slower at
every size. Heterogeneous eager stringify remains +0.434% versus main (9/9
slower); it does not use the lazy shortcut. Maximum median peak RSS increase
is 624 KiB on the 13 KiB roundtrip. No full-suite expansion is justified.
The earlier R1 window had 86.9–101.4% roundtrip slowdowns versus main; these
are separate recorded windows, not a paired R1/R2 experiment.
