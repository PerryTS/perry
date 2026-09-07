# Seven paired repeats

Medians of seven randomized two-arm pairs per case. CPU includes the whole loop. Ranges are observed minima/maxima, not confidence intervals.

| Fixture | Operation | Parent µs | Candidate µs | CPU change | CPU ranges separated | Peak RSS change MiB | Peak ranges separated |
|---|---|---:|---:|---:|---|---:|---|
| empty_object | parse | 0.057292 | 0.057156 | -0.237% | False | -0.031250 | True |
| empty_object | stringify | 0.046837 | 0.047219 | +0.816% | False | -0.046875 | True |
| escaped_1m | parse | 2040.280488 | 2040.475610 | +0.010% | False | +0.015625 | False |
| escaped_1m | stringify | 951.839286 | 951.642857 | -0.021% | False | +0.015625 | False |
| heterogeneous_1m | parse | 1934.875000 | 1934.425000 | -0.023% | False | +0.031250 | True |
| heterogeneous_1m | stringify | 848.279621 | 849.308057 | +0.121% | False | -0.031250 | True |
| long_string_1m | parse | 203.350153 | 202.838685 | -0.252% | False | +0.015625 | False |
| long_string_1m | stringify | 173.763717 | 173.755752 | -0.005% | False | +0.031250 | True |
| null | parse | 0.013756 | 0.013754 | -0.011% | False | +0.062500 | True |
| null | stringify | 0.006562 | 0.006566 | +0.046% | False | +0.015625 | False |
| numbers_1m | parse | 1738.358696 | 1738.586957 | +0.013% | False | +0.062500 | True |
| numbers_1m | stringify | 1604.854167 | 1606.572917 | +0.107% | False | -0.046875 | True |
| object_1k | parse | 0.542132 | 0.541204 | -0.171% | False | -0.046875 | False |
| object_1k | stringify | 0.237089 | 0.236966 | -0.052% | False | -0.046875 | True |
| records_array_16k | parse | 23.820893 | 23.845174 | +0.102% | False | +0.046875 | True |
| records_array_16k | stringify | 23.524400 | 23.412668 | -0.475% | False | -0.140625 | True |
| records_array_1m | parse | 1551.978947 | 1553.400000 | +0.092% | False | +0.015625 | False |
| records_array_1m | stringify | 1495.751412 | 1508.542373 | +0.855% | False | -0.078125 | False |
| records_array_20m | parse | 53404.000000 | 52971.000000 | -0.811% | False | -0.078125 | True |
| records_array_20m | stringify | 81805.375000 | 82092.625000 | +0.351% | False | -0.093750 | False |
| records_array_8m | parse | 13393.800000 | 13383.000000 | -0.081% | False | +0.546875 | True |
| records_array_8m | stringify | 12957.950000 | 13058.250000 | +0.774% | True | -0.015625 | True |
| records_object_1m | parse | 2835.042254 | 2820.098592 | -0.527% | False | -1.609375 | True |
| records_object_1m | stringify | 1498.724138 | 1511.229885 | +0.834% | True | -0.093750 | True |
| records_object_20m | parse | 53379.333333 | 52997.000000 | -0.716% | True | -0.078125 | True |
| records_object_20m | stringify | 81993.750000 | 82138.000000 | +0.176% | False | -0.109375 | True |
| records_object_8m | parse | 21618.000000 | 21451.375000 | -0.771% | True | -0.125000 | True |
| records_object_8m | stringify | 12907.238095 | 12960.047619 | +0.409% | False | -0.031250 | True |
| small_record | parse | 0.542261 | 0.540470 | -0.330% | False | -0.046875 | True |
| small_record | stringify | 0.283758 | 0.284218 | +0.162% | False | -0.062500 | False |
| string_a | parse | 0.016254 | 0.016256 | +0.009% | False | +0.062500 | True |
| string_a | stringify | 0.009694 | 0.009695 | +0.015% | False | +0.015625 | False |
| tiny_object | parse | 0.121568 | 0.121288 | -0.230% | False | -0.031250 | False |
| tiny_object | stringify | 0.092678 | 0.093423 | +0.803% | True | -0.062500 | True |
| unicode_1m | parse | 257.358553 | 257.117105 | -0.094% | False | +0.046875 | False |
| unicode_1m | stringify | 140.032670 | 138.588778 | -1.031% | False | +0.046875 | True |
| wide_1m | parse | 20452.388889 | 15190.555556 | -25.727% | True | -9.234375 | True |
| wide_1m | stringify | 1692.190698 | 1708.716279 | +0.977% | True | -0.046875 | False |
