# Linker-order diagnostic

Seven randomized, fresh-process quadruples per case. CPU includes user and system time. Negative changes mean less CPU time.

| Fixture | Operation | Stack-plan vs checkpoint, original | Stack-plan vs checkpoint, ordered | Ordered vs original checkpoint | Ordered vs original stack plan |
|---|---|---:|---:|---:|---:|
| tiny_object | stringify | -0.72% | -0.75% | +0.02% | -0.01% |
| small_record | stringify | -0.16% | -0.21% | +0.32% | +0.27% |
| records_array_16k | parse | +2.82% | +0.62% | +0.03% | -2.11% |
| records_array_1m | parse | +2.65% | +0.56% | -0.02% | -2.06% |
| records_array_8m | parse | +2.39% | +0.68% | -0.10% | -1.77% |
| numbers_1m | stringify | +2.31% | -0.16% | +0.02% | -2.40% |
| wide_1m | parse | +0.93% | -0.11% | +0.27% | -0.77% |
| heterogeneous_1m | parse | +2.74% | +0.31% | -0.02% | -2.38% |
