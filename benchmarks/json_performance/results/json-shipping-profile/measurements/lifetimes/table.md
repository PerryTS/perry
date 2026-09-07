# Lifetimes including final collection

Three randomized triples per case. Negative changes mean less total CPU. Ranges are observed minima/maxima, not confidence intervals.

| Fixture | Operation | Lifetime | Calls | Parent ms | Checkpoint ms | Candidate ms | vs checkpoint | vs parent |
|---|---|---|---:|---:|---:|---:|---:|---:|
| records_object_1m | stringify | discard | 120 | 184.496 | 179.291 | 178.459 | -0.46% | -3.27% |
| records_object_20m | roundtrip | discard | 7 | 1125.333 | 1062.703 | 1035.062 | -2.60% | -8.02% |
| records_object_20m | roundtrip | discard | 8 | 1289.489 | 1269.351 | 1232.584 | -2.90% | -4.41% |
| records_object_20m | roundtrip | discard | 9 | 1387.550 | 1365.287 | 1328.015 | -2.73% | -4.29% |
| records_object_20m | roundtrip | discard | 16 | 2431.452 | 2417.096 | 2363.607 | -2.21% | -2.79% |
| records_object_20m | roundtrip | discard | 24 | 3382.215 | 3621.247 | 3539.390 | -2.26% | +4.65% |
| small_record | stringify | discard | 100000 | 39.226 | 37.807 | 37.624 | -0.48% | -4.08% |
| small_record | stringify | latest | 100000 | 39.556 | 37.697 | 37.730 | +0.09% | -4.62% |
| small_record | stringify | retain | 100000 | 45.431 | 44.483 | 44.218 | -0.60% | -2.67% |
| wide_1m | parse | discard | 60 | 1226.949 | 1197.731 | 1198.085 | +0.03% | -2.35% |
| wide_1m | parse | latest | 60 | 1244.443 | 1215.675 | 1215.175 | -0.04% | -2.35% |
| wide_1m | parse | retain | 8 | 121.320 | 121.681 | 121.803 | +0.10% | +0.40% |
