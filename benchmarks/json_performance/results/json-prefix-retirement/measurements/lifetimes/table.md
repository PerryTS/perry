# Lifetimes including final collection

Three randomized triples per case. Negative changes mean less total CPU. Ranges are observed minima/maxima, not confidence intervals.

| Fixture | Operation | Lifetime | Calls | Parent ms | Checkpoint ms | Candidate ms | vs checkpoint | vs parent |
|---|---|---|---:|---:|---:|---:|---:|---:|
| records_object_1m | stringify | discard | 120 | 183.153 | 184.578 | 184.503 | -0.04% | +0.74% |
| records_object_20m | roundtrip | discard | 7 | 1130.919 | 1126.036 | 1106.052 | -1.77% | -2.20% |
| records_object_20m | roundtrip | discard | 8 | 1234.667 | 1289.292 | 1269.980 | -1.50% | +2.86% |
| records_object_20m | roundtrip | discard | 9 | 1447.825 | 1386.144 | 1367.471 | -1.35% | -5.55% |
| records_object_20m | roundtrip | discard | 16 | 2500.916 | 2429.939 | 2397.722 | -1.33% | -4.13% |
| records_object_20m | roundtrip | discard | 24 | 3681.721 | 3381.813 | 3343.611 | -1.13% | -9.18% |
| small_record | stringify | discard | 100000 | 38.133 | 39.200 | 38.919 | -0.72% | +2.06% |
| small_record | stringify | latest | 100000 | 38.268 | 39.311 | 38.201 | -2.82% | -0.18% |
| small_record | stringify | retain | 100000 | 44.808 | 45.587 | 45.236 | -0.77% | +0.96% |
| wide_1m | parse | discard | 60 | 2156.103 | 1226.301 | 1226.371 | +0.01% | -43.12% |
| wide_1m | parse | latest | 60 | 2213.655 | 1245.058 | 1246.122 | +0.09% | -43.71% |
| wide_1m | parse | retain | 8 | 121.315 | 120.954 | 121.128 | +0.14% | -0.15% |
