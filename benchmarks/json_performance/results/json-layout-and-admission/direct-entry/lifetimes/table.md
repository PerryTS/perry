**UNQUALIFIED: local observer failed with ENOSPC; end-of-run quiet gate did not pass. These raw results are diagnostic only.**

# Lifetimes including final collection

Three randomized triples per case. Negative changes mean less total CPU. Ranges are observed minima/maxima, not confidence intervals.

| Fixture | Operation | Lifetime | Calls | Parent ms | Checkpoint ms | Candidate ms | vs checkpoint | vs parent |
|---|---|---|---:|---:|---:|---:|---:|---:|
| records_object_1m | stringify | discard | 120 | 183.547 | 184.212 | 182.932 | -0.69% | -0.34% |
| records_object_20m | roundtrip | discard | 7 | 1131.212 | 1125.964 | 1077.454 | -4.31% | -4.75% |
| records_object_20m | roundtrip | discard | 8 | 1234.575 | 1289.128 | 1289.827 | +0.05% | +4.48% |
| records_object_20m | roundtrip | discard | 9 | 1448.948 | 1386.270 | 1383.945 | -0.17% | -4.49% |
| records_object_20m | roundtrip | discard | 16 | 2501.839 | 2430.351 | 2452.443 | +0.91% | -1.97% |
| records_object_20m | roundtrip | discard | 24 | 3674.562 | 3383.172 | 3669.694 | +8.47% | -0.13% |
| small_record | stringify | discard | 100000 | 38.058 | 39.244 | 37.250 | -5.08% | -2.12% |
| small_record | stringify | latest | 100000 | 38.152 | 39.395 | 37.452 | -4.93% | -1.83% |
| small_record | stringify | retain | 100000 | 44.533 | 45.679 | 43.854 | -4.00% | -1.52% |
| wide_1m | parse | discard | 60 | 2156.077 | 1224.678 | 1227.257 | +0.21% | -43.08% |
| wide_1m | parse | latest | 60 | 2212.328 | 1243.415 | 1243.079 | -0.03% | -43.81% |
| wide_1m | parse | retain | 8 | 121.155 | 121.148 | 121.536 | +0.32% | +0.31% |
