# Longer original-worker replay: opening scan R1

Reference is corrected R2 from PR #10034. Eight previously slower original-matrix rows, fixed counts, nine randomized paired repetitions. The quiet window and exact runners are archived. Every checksum and Node output comparison passed.

| Fixture / operation | Candidate µs | Reference µs | CPU change | Sample ranges | Peak change KiB |
|---|---:|---:|---:|---|---:|
| null / parse | 0.011259050 | 0.010638450 | +5.834% | separated slower | +144 |
| string_a / parse | 0.013450350 | 0.012878800 | +4.438% | separated slower | +144 |
| empty_object / parse | 0.020768300 | 0.020430600 | +1.653% | overlap | +112 |
| tiny_object / parse | 0.034855000 | 0.034575000 | +0.810% | separated slower | +96 |
| small_record / parse | 0.097387000 | 0.097025000 | +0.373% | overlap | +96 |
| object_1k / parse | 0.082815000 | 0.082466500 | +0.423% | overlap | +96 |
| records_array_16k / sparse | 25.104600000 | 25.025700000 | +0.315% | overlap | +128 |
| numbers_1m / stringify | 999.484000000 | 994.356000000 | +0.516% | overlap | +112 |

These results are for the original worker and its repeat-source paths. The separate rotating suite showed different scalar behavior. R1 remains experimental; R2 tests outlining empty allocation from the shared parse entry.
