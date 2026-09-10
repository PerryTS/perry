# Lazy canonical R1 focused replay

Nine repetitions per case and three randomized Perry arms: lazy canonical R1, freshly built merged main eee3881c4, and tape-depth R2. Node is the output oracle. All 20 output checks and 135 timing trials passed, under the archived quiet-host window. CPU is process CPU per call, with all samples retained. Differences are descriptive; overlapping ranges do not prove equality.

| Fixture | Operation | Candidate us | Main us | R2 us | vs main | vs R2 | Slower pairs | Peak delta KiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| records_array_16k | roundtrip | 50.075100 | 24.857400 | 20.521200 | +101.449% | +144.016% | 9/9 | -336 |
| records_array_1m | roundtrip | 3252.863281 | 1681.738281 | 1397.289062 | +93.423% | +132.798% | 9/9 | +112 |
| records_array_8m | roundtrip | 26804.312500 | 14339.375000 | 12237.171875 | +86.928% | +119.040% | 9/9 | +144 |
| heterogeneous_1m | stringify | 677.572266 | 674.694336 | 677.945312 | +0.427% | -0.055% | 9/9 | +128 |
| small_record | parse | 0.096489 | 0.097039 | 0.096638 | -0.567% | -0.154% | 0/9 | +128 |

R1 is rejected on CPU cost. Record roundtrips are 86.9–101.4% slower than main
and 119.0–144.0% slower than R2, with 9/9 main pairs slower on every size.
Heterogeneous eager stringify remains +0.427% versus main (9/9 slower). This
admission guard does not run on that eager workload. No full-suite R1 expansion
is justified. The correction passes 297 tests and 126 compiled candidate
canonicalization/GC comparisons, but its separate token proof is too expensive.

The quiet window retains the controller’s filtered competing-process lists.
Its full process listings were not copied before the next run; R2 and future
archives also retain those full listings.
