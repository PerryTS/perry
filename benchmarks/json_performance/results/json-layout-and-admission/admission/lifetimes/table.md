# Lifetimes including final collection

Three randomized triples per case. Negative changes mean less total CPU. Ranges are observed minima/maxima, not confidence intervals.

| Fixture | Operation | Lifetime | Calls | Parent ms | Checkpoint ms | Candidate ms | vs checkpoint | vs parent |
|---|---|---|---:|---:|---:|---:|---:|---:|
| records_object_1m | stringify | discard | 120 | 182.881 | 184.786 | 183.507 | -0.69% | +0.34% |
| records_object_20m | roundtrip | discard | 7 | 1130.608 | 1124.487 | 1078.446 | -4.09% | -4.61% |
| records_object_20m | roundtrip | discard | 8 | 1234.579 | 1289.195 | 1290.594 | +0.11% | +4.54% |
| records_object_20m | roundtrip | discard | 9 | 1450.361 | 1386.420 | 1383.355 | -0.22% | -4.62% |
| records_object_20m | roundtrip | discard | 16 | 2507.088 | 2430.307 | 2451.212 | +0.86% | -2.23% |
| records_object_20m | roundtrip | discard | 24 | 3681.511 | 3383.299 | 3674.125 | +8.60% | -0.20% |
| small_record | stringify | discard | 100000 | 38.046 | 39.185 | 37.291 | -4.83% | -1.98% |
| small_record | stringify | latest | 100000 | 38.355 | 39.547 | 37.537 | -5.08% | -2.13% |
| small_record | stringify | retain | 100000 | 44.674 | 45.510 | 43.955 | -3.42% | -1.61% |
| wide_1m | parse | discard | 60 | 2157.391 | 1224.865 | 1224.995 | +0.01% | -43.22% |
| wide_1m | parse | latest | 60 | 2221.531 | 1243.716 | 1243.846 | +0.01% | -44.01% |
| wide_1m | parse | retain | 8 | 122.028 | 121.191 | 121.348 | +0.13% | -0.56% |
