All 38 paired cases against record-bytes. Seven fresh candidate and seven fresh reference processes per case. CPU includes user and system time across the benchmark loop. Observed range separation is descriptive, not a confidence interval.

| Fixture | Operation | Previous µs | Batch µs | CPU change | CPU ranges separate | Peak RSS change MiB | Peak ranges separate |
|---|---|---:|---:|---:|---|---:|---|
| empty_object | parse | 0.057707 | 0.057244 | -0.801% | True | -0.093750 | True |
| empty_object | stringify | 0.047274 | 0.046803 | -0.995% | True | -0.015625 | False |
| escaped_1m | parse | 2042.536585 | 2038.670732 | -0.189% | False | -0.156250 | True |
| escaped_1m | stringify | 950.112426 | 950.479290 | +0.039% | False | -0.109375 | True |
| heterogeneous_1m | parse | 1911.148148 | 1932.679012 | +1.127% | True | -0.187500 | True |
| heterogeneous_1m | stringify | 838.535545 | 845.573460 | +0.839% | True | -0.031250 | False |
| long_string_1m | parse | 204.368539 | 204.122097 | -0.121% | False | -0.156250 | True |
| long_string_1m | stringify | 172.662681 | 174.079419 | +0.821% | False | -0.109375 | False |
| null | parse | 0.013760 | 0.013758 | -0.018% | False | -0.109375 | True |
| null | stringify | 0.006568 | 0.006563 | -0.076% | False | -0.062500 | True |
| numbers_1m | parse | 1712.021978 | 1712.802198 | +0.046% | False | -0.156250 | True |
| numbers_1m | stringify | 1606.229167 | 1605.479167 | -0.047% | False | +2.078125 | True |
| object_1k | parse | 0.593689 | 0.542378 | -8.643% | True | -0.171875 | True |
| object_1k | stringify | 0.233120 | 0.237607 | +1.925% | True | -0.078125 | True |
| records_array_16k | parse | 23.772835 | 23.807051 | +0.144% | False | -0.187500 | True |
| records_array_16k | stringify | 23.583383 | 23.342147 | -1.023% | True | -0.031250 | False |
| records_array_1m | parse | 1551.854167 | 1553.770833 | +0.124% | False | -0.203125 | True |
| records_array_1m | stringify | 1500.363636 | 1499.681818 | -0.045% | False | -0.093750 | True |
| records_array_20m | parse | 75260.000000 | 53308.333333 | -29.168% | True | +0.312500 | True |
| records_array_20m | stringify | 81607.125000 | 81822.375000 | +0.264% | False | -0.125000 | True |
| records_array_8m | parse | 13374.500000 | 13379.200000 | +0.035% | False | -0.187500 | True |
| records_array_8m | stringify | 12931.619048 | 12856.476190 | -0.581% | False | -0.015625 | False |
| records_object_1m | parse | 3936.859155 | 2835.309859 | -27.980% | True | -0.125000 | False |
| records_object_1m | stringify | 1500.694915 | 1494.451977 | -0.416% | False | -0.062500 | True |
| records_object_20m | parse | 75734.333333 | 53275.666667 | -29.655% | True | +0.312500 | True |
| records_object_20m | stringify | 81606.875000 | 81857.375000 | +0.307% | False | -0.093750 | True |
| records_object_8m | parse | 30692.875000 | 21625.250000 | -29.543% | True | -0.062500 | False |
| records_object_8m | stringify | 12936.380952 | 12881.761905 | -0.422% | False | +0.015625 | True |
| small_record | parse | 0.670859 | 0.543362 | -19.005% | True | -0.093750 | True |
| small_record | stringify | 0.280312 | 0.283030 | +0.970% | True | -0.062500 | True |
| string_a | parse | 0.016256 | 0.016255 | -0.003% | False | -0.093750 | True |
| string_a | stringify | 0.009692 | 0.009694 | +0.010% | False | -0.062500 | True |
| tiny_object | parse | 0.121143 | 0.121917 | +0.640% | True | -0.093750 | True |
| tiny_object | stringify | 0.090657 | 0.092762 | +2.322% | False | -0.046875 | True |
| unicode_1m | parse | 256.205321 | 257.184857 | +0.382% | False | -0.156250 | False |
| unicode_1m | stringify | 138.274101 | 138.256115 | -0.013% | False | -0.109375 | True |
| wide_1m | parse | 20853.027778 | 20391.638889 | -2.213% | True | -0.140625 | False |
| wide_1m | stringify | 1694.452830 | 1693.457547 | -0.059% | False | +0.031250 | True |
