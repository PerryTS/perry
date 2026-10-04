# Private deferred RSS query: current07 full17

UNAPPLIED. 204 correct fresh executions, 15 exact production controls and 15 fresh candidate builds. Default GC and normal automatic runtime optimization. Three RSS and three instruction samples per case/arm; distributions and raw proofs are bound in the JSON artifacts.

| Case | RSS vs GC | Instructions vs GC | Minor faults vs GC |
|---|---:|---:|---:|
| 12_large_live_set | +0.233% | -0.014% | +0.009% |
| 14_grow_then_churn | +0.139% | -0.058% | -0.011% |
| 23-binary-trees | -0.039% | -0.021% | +0.003% |
| 30-string-build | +0.006% | -0.004% | +0.000% |
| 31-json | -0.104% | -0.002% | +0.002% |
| 51-pipeline | -0.002% | -0.043% | +0.007% |
| 60-ring-churn | +1.041% | -0.109% | +0.221% |
| 71-async-worker | -0.143% | -0.016% | +0.000% |
| 72-request-processing | +0.354% | -0.366% | -0.134% |
| 73-retain-then-release | +0.325% | +0.602% | +0.001% |
| 70-documents-replace | +0.314% | +0.016% | +2.115% |
| 70-documents-retain | -0.823% | -0.008% | -0.023% |
| 70-documents-transition | -0.091% | +0.006% | +0.053% |
| tscwork | -0.604% | +0.043% | +1.641% |
| zodwork | +0.729% | -0.021% | +0.000% |
| 00-noop | +0.000% | -1.208% | -1.852% |
| 15-crc32 | -1.288% | -0.001% | +0.079% |

Removing an unused RSS query is proven by the negative control and runtime tests. This matrix does not show a uniform application instruction or RSS gain: retain/release instructions +0.602%, TS instructions +0.043%, Zod RSS +0.729%. No adoption claimed. The linked checks independently pass 44 executions, all 22 moving runs actually copying and protecting retired from-space.
