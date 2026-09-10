# Tape-depth R3 focused replay

Nine repetitions per case, with 27 for Unicode stringify, and three randomized Perry arms: tape-depth R3, freshly built merged main eee3881c4, and tape-depth R2. Node is the output oracle. All 52 output checks and 405 timing trials passed, under the archived quiet-host window. CPU is process CPU per call, with all samples retained. Differences are descriptive; overlapping ranges do not prove equality.

| Fixture | Operation | Candidate us | Main us | R2 us | vs main | vs R2 | Slower pairs | Peak delta KiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| records_array_16k | parse | 13.622650 | 18.101200 | 13.632850 | -24.742% | -0.075% | 0/9 | +64 |
| records_array_16k | sparse | 21.288200 | 25.139400 | 21.288800 | -15.319% | -0.003% | 0/9 | +96 |
| records_array_16k | scan | 72.149500 | 76.880600 | 72.221900 | -6.154% | -0.100% | 0/9 | +128 |
| records_array_1m | parse | 907.994141 | 1189.507812 | 906.847656 | -23.666% | +0.126% | 0/9 | +64 |
| heterogeneous_1m | parse | 1112.003906 | 1390.982422 | 1112.078125 | -20.056% | -0.007% | 0/9 | +80 |
| small_record | parse | 0.096501 | 0.096998 | 0.096527 | -0.513% | -0.027% | 0/9 | +160 |
| small_record | stringify | 0.043012 | 0.042983 | 0.043034 | +0.067% | -0.053% | 6/9 | +192 |
| long_string_1m | stringify | 32.051849 | 31.662994 | 31.690765 | +1.228% | +1.139% | 5/9 | +176 |
| unicode_1m | stringify | 26.025269 | 26.702209 | 25.961212 | -2.535% | +0.247% | 13/27 | +160 |
| unicode_1m | parse | 0.371644 | 0.370829 | 0.371651 | +0.220% | -0.002% | 7/9 | +160 |
| tiny_object | parse | 0.034068 | 0.034589 | 0.034076 | -1.505% | -0.022% | 0/9 | +176 |
| wide_1m | parse | 2868.609375 | 2870.453125 | 2869.320312 | -0.064% | -0.025% | 4/9 | +160 |
| heterogeneous_1m | stringify | 686.145508 | 673.988281 | 677.652344 | +1.804% | +1.253% | 9/9 | +112 |
