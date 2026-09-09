# Opening scan R3: longer original-worker replay

Quiet M1/8 GiB window 2026-09-09 20:36:11–20:37:30 UTC. Ten cases, nine randomized paired repetitions, three Perry arms: R3, corrected R2 baseline, and scanner R2 prior. All 270 timing trials and 40 output-oracle runs passed. This worker repeats one source; changing-input validation is separate.

| Fixture / operation | R3 µs | Reference µs | CPU change | Sample ranges | Change vs scanner R2 |
|---|---:|---:|---:|---|---:|
| null / parse | 0.010635000 | 0.010636850 | -0.017% | overlap | +0.031% |
| string_a / parse | 0.012829500 | 0.012879050 | -0.385% | separated faster | -0.011% |
| empty_object / parse | 0.019856100 | 0.020442400 | -2.868% | separated faster | +0.033% |
| tiny_object / parse | 0.034068400 | 0.034606200 | -1.554% | separated faster | -0.019% |
| small_record / parse | 0.095855000 | 0.096966500 | -1.146% | separated faster | -0.040% |
| object_1k / parse | 0.081368500 | 0.082466000 | -1.331% | separated faster | +0.045% |
| records_array_16k / sparse | 25.087800000 | 25.044600000 | +0.172% | overlap | -0.001% |
| numbers_1m / stringify | 994.672000000 | 994.384000000 | +0.029% | overlap | -0.051% |
| heterogeneous_1m / parse | 1401.113281250 | 1396.285156250 | +0.346% | overlap | -0.050% |
| wide_1m / parse | 2868.710937500 | 2872.070312500 | -0.117% | overlap | +0.013% |

No row has separated slower ranges in this replay. Sparse and heterogeneous medians remain +0.172%/+0.346% above reference and are almost unchanged from scanner R2 in the same run. This does not establish that outlining repaired those regressions; the full matrices remain necessary. Maximum median peak-RSS increase is 160 KiB.

Exact runners, raw trials, quiet admission, source patch, artifact hashes and GC witnesses are archived beside this report.
