# Learned lower nursery floor: rejected

Private 3fb6cd50ed versus exact deferred-query control d4f491673b, main07. All 204 executions match Node; 15 exact control and 15 fresh candidate builds. Normal automatic runtime optimization, default GC, three RSS and three instruction samples per case/arm. Mac 4,879/Linux 4,863 runtime tests and all 44 linked executions pass; all 22 moving runs actually copying/protected. Correctness does not justify adoption.

| Case | RSS vs control | Instructions vs control | Minor faults vs control |
|---|---:|---:|---:|---:|
| 12_large_live_set | -0.187% | -0.014% | +0.009% |
| 14_grow_then_churn | +0.124% | +0.013% | -0.007% |
| 23-binary-trees | -0.035% | -0.001% | -0.001% |
| 30-string-build | +0.080% | -0.029% | -0.420% |
| 31-json | +0.046% | +0.049% | +1.841% |
| 51-pipeline | +0.064% | -0.032% | +0.036% |
| 60-ring-churn | +0.770% | +4.112% | +0.277% |
| 71-async-worker | -0.159% | -0.005% | -0.020% |
| 72-request-processing | +0.293% | -0.484% | +0.072% |
| 73-retain-then-release | +0.441% | -0.019% | +0.923% |
| 70-documents-replace | +0.211% | -0.020% | -1.878% |
| 70-documents-retain | -0.781% | -0.025% | -0.568% |
| 70-documents-transition | -0.059% | -0.027% | -0.046% |
| tscwork | +0.148% | -0.022% | -1.479% |
| zodwork | -0.841% | +0.412% | +0.346% |
| 00-noop | +0.000% | +0.866% | +1.852% |
| 15-crc32 | -0.667% | -0.001% | +0.000% |

Rejected for adoption: ring churn instructions +4.112% and peak RSS +0.770%; TypeScript RSS +0.148%. Lowering the learned nursery cap does not materially reduce these peaks. Initial peak occupancy and retained warm mappings are possible limits, not proven explanations from this matrix. Natural trace showed low survival, which justified the experiment but was insufficient to predict a peak-RSS win. Both policy source and all signed raw results remain preserved.
