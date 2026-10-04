Private native-map prefix experiment f27fcd49d8 versus adopted once-advice 8e3013733d on main07. All 204 executions correct; all 17 cases and raw source/product/oracle/counter bindings independently verified. Fifteen control builds reused; twelve completed candidate builds reused after the preserved disk-guard stop, three remaining candidates built. No workload or GC default changed.

Decision: rejected for adoption. Avoiding suffix decoding is proven in actual decoder tests, but this matrix does not establish material application benefit. TypeScript instructions −0.0458%, peak RSS +0.0383%; requests instructions +0.7667%; ring RSS +1.7986%. Three samples per metric/arm do not establish persistent small regressions. Complete signed distributions remain in the companion summary; no row is excluded.

| Case | Peak RSS Δ | Instructions Δ | Minor faults Δ |
|---|---:|---:|---:|
| 12_large_live_set | +0.2940% | -0.0177% | +0.0260% |
| 14_grow_then_churn | -0.1797% | +0.0223% | +0.0035% |
| 23-binary-trees | +0.0347% | +0.0002% | +0.0027% |
| 30-string-build | +0.0445% | -0.0179% | -0.4195% |
| 31-json | -0.1753% | +0.0039% | +0.0057% |
| 51-pipeline | +0.0661% | -0.0314% | +0.0227% |
| 60-ring-churn | +1.7986% | +0.0783% | +0.2776% |
| 71-async-worker | -0.0467% | +0.0039% | +0.0050% |
| 72-request-processing | +0.0488% | +0.7667% | +0.0724% |
| 73-retain-then-release | +0.3614% | -0.0416% | +0.2347% |
| 70-documents-replace | -0.2648% | +0.0060% | +0.0507% |
| 70-documents-retain | +0.2064% | -0.0846% | -2.1461% |
| 70-documents-transition | -0.0664% | -0.0744% | -2.0387% |
| tscwork | +0.0383% | -0.0458% | -0.4497% |
| zodwork | -0.3779% | +0.0079% | +0.1153% |
| 00-noop | +0.3106% | +1.1289% | +0.0000% |
| 15-crc32 | -0.0499% | +0.0005% | +0.0791% |
