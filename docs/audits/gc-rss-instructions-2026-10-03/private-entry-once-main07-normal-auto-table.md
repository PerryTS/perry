# Private once-per-idle-interval advice: current07 full17

Private commit `8e3013733d`; production GC control `4f9121c09b`; base `07e50b14a3`. UNAPPLIED. All 306 fresh executions matched Node 26.5.1. Fifteen candidate builds and 30 exact control builds; normal automatic runtime optimization; no GC overrides. Three RSS and three instruction samples per case/arm. Raw receipts and distributions are bound by the verification JSON.

| Case | RSS vs GC | Instructions vs GC | Minor faults vs GC |
|---|---:|---:|---:|
| 12_large_live_set | +0.057% | -0.123% | -0.009% |
| 14_grow_then_churn | -0.176% | +0.053% | +0.000% |
| 23-binary-trees | -11.705% | +0.010% | +1.156% |
| 30-string-build | +0.000% | -0.234% | -0.008% |
| 31-json | +0.051% | -0.137% | -0.002% |
| 51-pipeline | -0.023% | -0.130% | -0.004% |
| 60-ring-churn | -1.056% | -0.028% | -0.497% |
| 71-async-worker | -0.965% | -0.243% | +0.035% |
| 72-request-processing | -0.463% | -0.320% | -0.072% |
| 73-retain-then-release | +1.338% | -0.062% | -0.398% |
| 70-documents-replace | -0.312% | -0.178% | -0.052% |
| 70-documents-retain | -0.945% | -0.170% | -0.122% |
| 70-documents-transition | -0.103% | -0.174% | -0.091% |
| tscwork | -2.159% | -1.474% | +4.470% |
| zodwork | -0.016% | -0.001% | +0.087% |
| 00-noop | +0.000% | -0.649% | +0.000% |
| 15-crc32 | -0.174% | +0.003% | +0.040% |

The retain/release median RSS increase of 1.34% remains an adoption concern. Nine fresh RSS repeats per arm on the exact same binaries are queued; this table preserves the original measurements. Once advice is still private.

Linked correctness independently passed 44 executions across 11 fixtures; all 22 moving runs copied objects and protected retired from-space. This does not establish an RSS benefit.
