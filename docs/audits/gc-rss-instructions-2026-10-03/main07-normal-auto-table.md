# Main 07e50b14a3 application measurements

45 fresh builds; 306 correct measurements; three RSS and three perf repetitions per arm and case. All raw evidence independently verified and 749 files copied locally and rehashed. Values below are median changes; complete distributions are in main07-normal-auto-summary.json. Private entry is unapplied. This checkpoint supersedes preceding-main results for these sources, without changing the historical records.

| Workload | Production RSS % | Production instructions % | Private entry RSS % | Private entry instructions % |
|---|---:|---:|---:|---:|
| 12_large_live_set | -6.594 | -0.897 | -0.029 | -0.072 |
| 14_grow_then_churn | -5.704 | -1.519 | +0.158 | +0.067 |
| 23-binary-trees | -0.199 | -0.272 | -11.665 | +0.010 |
| 30-string-build | -1.824 | -0.001 | -0.019 | -0.217 |
| 31-json | -3.628 | +0.164 | +0.020 | -0.085 |
| 51-pipeline | -10.795 | -0.666 | +0.055 | -0.098 |
| 60-ring-churn | -0.379 | -0.041 | -0.353 | -0.011 |
| 71-async-worker | -1.513 | -1.261 | -1.414 | -0.025 |
| 72-request-processing | -0.588 | -5.599 | +0.134 | -0.734 |
| 73-retain-then-release | -9.663 | +0.065 | +0.282 | -0.132 |
| 70-documents-replace | -9.516 | -0.301 | +0.116 | -0.169 |
| 70-documents-retain | -12.895 | -1.076 | +0.014 | -0.095 |
| 70-documents-transition | -9.890 | -0.738 | -0.062 | -0.173 |
| tscwork | -2.029 | +0.681 | -2.306 | -1.459 |
| zodwork | +0.181 | +0.360 | -0.591 | +0.058 |
| 00-noop | +0.000 | -0.476 | +0.000 | +0.152 |
| 15-crc32 | -3.732 | -0.008 | +1.093 | -0.000 |

Production TS minor faults +38.758%; private entry adds +3.556% versus production. RSS improves in 15 production cases, no-op is flat, Zod rises 0.181%. Production instruction regressions remain visible: TS +0.681%, Zod +0.360%, JSON +0.164%, retain/release +0.065%. Private entry improves TS instructions on this checkpoint (-1.459%), unlike preceding main (+1.979%); no source-independent or uniform benefit is claimed.
