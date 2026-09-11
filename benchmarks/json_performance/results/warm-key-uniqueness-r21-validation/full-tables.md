# R21 complete CPU and peak RSS tables

Same 50-row window: original38 parse/stringify plus12 consumption. Seven interleaved fresh-process repetitions per engine. CPU is user plus system microseconds per operation; RSS is median whole-process peak MiB. These repeated-input cases are supplemented by rotating-input and retained-output checks before acceptance. R21 is parked because small-record stringify remains slightly slower across three windows; no acceptance or complete qualification is claimed.

## Original 38 parse/stringify rows — CPU

| Fixture / operation | Main µs | Candidate µs | Node µs | Bun µs | CPU delta | Slower pairs | Ranges |
|---|---:|---:|---:|---:|---:|---:|---|
| null / parse | 0.01063 | 0.01064 | 0.02736 | 0.01918 | +0.024% | 4/7 | Overlap |
| null / stringify | 0.00657 | 0.00657 | 0.02721 | 0.03124 | +0.030% | 5/7 | Overlap |
| string_a / parse | 0.01282 | 0.01283 | 0.03172 | 0.02258 | +0.019% | 4/7 | Overlap |
| string_a / stringify | 0.00969 | 0.00969 | 0.03008 | 0.03199 | +0.026% | 5/7 | Overlap |
| empty_object / parse | 0.02343 | 0.02340 | 0.04517 | 0.02416 | -0.128% | 3/7 | Overlap |
| empty_object / stringify | 0.01760 | 0.01759 | 0.03199 | 0.03257 | -0.062% | 2/7 | Overlap |
| tiny_object / parse | 0.03679 | 0.03678 | 0.08133 | 0.04521 | -0.035% | 3/7 | Overlap |
| tiny_object / stringify | 0.03395 | 0.03397 | 0.03716 | 0.04314 | +0.052% | 3/7 | Overlap |
| small_record / parse | 0.09867 | 0.09854 | 0.34988 | 0.24446 | -0.131% | 2/7 | Overlap |
| small_record / stringify | 0.04573 | 0.04599 | 0.10792 | 0.11924 | +0.553% | 6/7 | Overlap |
| object_1k / parse | 0.08195 | 0.08200 | 0.53331 | 0.22752 | +0.053% | 5/7 | Overlap |
| object_1k / stringify | 0.11512 | 0.11535 | 0.19399 | 0.21109 | +0.205% | 4/7 | Overlap |
| records_array_16k / parse | 13.58596 | 13.59376 | 40.20622 | 33.82214 | +0.057% | 4/7 | Overlap |
| records_array_16k / stringify | 11.01699 | 10.97019 | 13.13275 | 23.20959 | -0.425% | 1/7 | Overlap |
| records_array_1m / parse | 918.42353 | 918.65882 | 2718.97647 | 2141.60000 | +0.026% | 4/7 | Overlap |
| records_array_1m / stringify | 646.43668 | 645.93013 | 849.21834 | 966.57642 | -0.078% | 2/7 | Overlap |
| records_object_1m / parse | 2008.98765 | 1975.56790 | 2833.59259 | 2151.75309 | -1.664% | 0/7 | Separated gain |
| records_object_1m / stringify | 649.62882 | 650.81223 | 845.59389 | 965.86463 | +0.182% | 5/7 | Overlap |
| records_array_8m / parse | 8284.25000 | 8266.87500 | 33552.12500 | 20763.62500 | -0.210% | 1/7 | Overlap |
| records_array_8m / stringify | 4729.44828 | 4723.65517 | 7251.24138 | 8204.86207 | -0.122% | 4/7 | Overlap |
| records_object_8m / parse | 15933.90000 | 15600.80000 | 33748.10000 | 20818.10000 | -2.091% | 0/7 | Separated gain |
| records_object_8m / stringify | 4774.13793 | 4773.93103 | 7273.44828 | 8173.79310 | -0.004% | 3/7 | Overlap |
| records_array_20m / parse | 39411.00000 | 38653.75000 | 94928.25000 | 53078.50000 | -1.921% | 0/7 | Separated gain |
| records_array_20m / stringify | 11744.25000 | 11753.08333 | 17298.00000 | 20051.83333 | +0.075% | 4/7 | Overlap |
| records_object_20m / parse | 39425.50000 | 38705.25000 | 95017.50000 | 53173.75000 | -1.827% | 0/7 | Separated gain |
| records_object_20m / stringify | 11707.58333 | 11705.66667 | 17264.00000 | 20090.41667 | -0.016% | 2/7 | Overlap |
| numbers_1m / parse | 1582.09804 | 1550.42157 | 3129.47059 | 3200.54902 | -2.002% | 0/7 | Separated gain |
| numbers_1m / stringify | 1014.37415 | 1013.50340 | 1974.31293 | 2942.10884 | -0.086% | 2/7 | Overlap |
| long_string_1m / parse | 0.34925 | 0.35139 | 364.47078 | 64.64255 | +0.612% | 6/7 | Overlap |
| long_string_1m / stringify | 35.90062 | 35.80229 | 102.47451 | 94.80125 | -0.274% | 2/7 | Overlap |
| escaped_1m / parse | 980.76582 | 980.18354 | 1763.16456 | 2072.00633 | -0.059% | 4/7 | Overlap |
| escaped_1m / stringify | 885.56024 | 885.06627 | 1898.06627 | 2088.63855 | -0.056% | 3/7 | Overlap |
| unicode_1m / parse | 0.34996 | 0.34889 | 441.17803 | 57.95262 | -0.308% | 1/7 | Overlap |
| unicode_1m / stringify | 28.36889 | 29.36364 | 409.84444 | 447.77495 | +3.506% | 3/7 | Overlap |
| wide_1m / parse | 2887.28125 | 2871.09375 | 5261.39062 | 4145.26562 | -0.561% | 0/7 | Overlap |
| wide_1m / stringify | 633.40948 | 633.80172 | 6352.44828 | 664.20690 | +0.062% | 5/7 | Overlap |
| heterogeneous_1m / parse | 1117.14789 | 1116.82394 | 3829.02113 | 2949.83099 | -0.029% | 3/7 | Overlap |
| heterogeneous_1m / stringify | 718.26606 | 719.60092 | 898.68349 | 1055.99541 | +0.186% | 5/7 | Overlap |

## Original 38 parse/stringify rows — peak RSS

| Fixture / operation | Main MiB | Candidate MiB | Node MiB | Bun MiB | RSS delta MiB |
|---|---:|---:|---:|---:|---:|
| null / parse | 12.7969 | 12.7969 | 57.7031 | 35.7500 | +0.0000 |
| null / stringify | 13.0000 | 13.0000 | 59.6719 | 129.8281 | +0.0000 |
| string_a / parse | 12.7969 | 12.7969 | 57.7188 | 36.1094 | +0.0000 |
| string_a / stringify | 13.0000 | 13.0000 | 59.6719 | 129.8125 | +0.0000 |
| empty_object / parse | 32.1250 | 32.1562 | 59.5625 | 69.0625 | +0.0312 |
| empty_object / stringify | 13.0938 | 13.0938 | 59.6406 | 129.8125 | +0.0000 |
| tiny_object / parse | 32.2969 | 32.2969 | 59.5938 | 69.0781 | +0.0000 |
| tiny_object / stringify | 32.3438 | 32.3281 | 59.6875 | 129.7969 | -0.0156 |
| small_record / parse | 79.5469 | 79.5625 | 59.7188 | 79.8906 | +0.0156 |
| small_record / stringify | 33.3281 | 33.3594 | 59.7656 | 378.7031 | +0.0312 |
| object_1k / parse | 64.9375 | 64.9688 | 61.8594 | 71.2812 | +0.0312 |
| object_1k / stringify | 33.3438 | 33.3750 | 61.8594 | 70.8594 | +0.0312 |
| records_array_16k / parse | 63.4219 | 63.4375 | 65.8438 | 70.6406 | +0.0156 |
| records_array_16k / stringify | 33.5156 | 33.5469 | 61.8281 | 71.0625 | +0.0312 |
| records_array_1m / parse | 67.0938 | 67.1094 | 125.6250 | 88.7344 | +0.0156 |
| records_array_1m / stringify | 63.0000 | 63.0625 | 129.3906 | 103.7188 | +0.0625 |
| records_object_1m / parse | 66.4219 | 66.4531 | 92.9531 | 79.1719 | +0.0312 |
| records_object_1m / stringify | 63.0469 | 63.0625 | 129.4688 | 103.6719 | +0.0156 |
| records_array_8m / parse | 108.8125 | 108.8281 | 266.0000 | 140.9375 | +0.0156 |
| records_array_8m / stringify | 121.1250 | 121.1562 | 212.5625 | 196.7812 | +0.0312 |
| records_object_8m / parse | 187.3125 | 187.3438 | 244.3906 | 131.7969 | +0.0312 |
| records_object_8m / stringify | 121.1719 | 121.1719 | 212.5469 | 176.7188 | +0.0000 |
| records_array_20m / parse | 255.4062 | 255.4531 | 359.9688 | 220.7969 | +0.0469 |
| records_array_20m / stringify | 235.2031 | 235.2188 | 459.5469 | 304.9531 | +0.0156 |
| records_object_20m / parse | 255.4219 | 255.4531 | 359.9219 | 220.7344 | +0.0312 |
| records_object_20m / stringify | 235.2188 | 235.2500 | 459.4844 | 304.9531 | +0.0312 |
| numbers_1m / parse | 62.7656 | 62.7812 | 104.1562 | 68.2344 | +0.0156 |
| numbers_1m / stringify | 57.8281 | 57.8594 | 113.0938 | 74.2656 | +0.0312 |
| long_string_1m / parse | 16.4219 | 16.4375 | 209.1250 | 164.4844 | +0.0156 |
| long_string_1m / stringify | 54.4688 | 54.5000 | 183.6562 | 153.6250 | +0.0312 |
| escaped_1m / parse | 58.3906 | 58.4062 | 102.6406 | 67.8906 | +0.0156 |
| escaped_1m / stringify | 54.8750 | 54.9375 | 124.8906 | 74.7969 | +0.0625 |
| unicode_1m / parse | 15.7500 | 15.7656 | 203.8594 | 159.8125 | +0.0156 |
| unicode_1m / stringify | 53.8438 | 53.9062 | 186.3125 | 156.2500 | +0.0625 |
| wide_1m / parse | 227.7031 | 227.7188 | 121.1875 | 95.0781 | +0.0156 |
| wide_1m / stringify | 68.6562 | 68.6875 | 117.9844 | 84.6875 | +0.0312 |
| heterogeneous_1m / parse | 62.5469 | 62.5625 | 92.2500 | 99.5312 | +0.0156 |
| heterogeneous_1m / stringify | 61.7031 | 61.7500 | 129.0312 | 104.0938 | +0.0469 |

## 12 consumption rows — CPU

| Fixture / operation | Main µs | Candidate µs | Node µs | Bun µs | CPU delta | Slower pairs | Ranges |
|---|---:|---:|---:|---:|---:|---:|---|
| records_array_16k / sparse | 19.96899 | 20.03673 | 39.67984 | 33.94306 | +0.339% | 7/7 | Overlap |
| records_array_16k / scan | 58.78968 | 57.87610 | 40.82372 | 35.69184 | -1.554% | 0/7 | Separated gain |
| records_array_16k / roundtrip | 20.96932 | 20.92440 | 54.97506 | 57.70571 | -0.214% | 1/7 | Overlap |
| records_array_1m / sparse | 966.49689 | 966.20497 | 2659.35404 | 2183.31677 | -0.030% | 3/7 | Overlap |
| records_array_1m / scan | 2933.52174 | 2897.86957 | 2931.26087 | 2228.84058 | -1.215% | 0/7 | Separated gain |
| records_array_1m / roundtrip | 1412.44348 | 1410.95652 | 3539.21739 | 3106.80870 | -0.105% | 1/7 | Overlap |
| records_array_8m / sparse | 8862.53333 | 8857.73333 | 32204.13333 | 21518.00000 | -0.054% | 3/7 | Overlap |
| records_array_8m / scan | 22674.87500 | 22325.00000 | 29767.00000 | 21343.87500 | -1.543% | 0/7 | Separated gain |
| records_array_8m / roundtrip | 13114.00000 | 13094.00000 | 37002.81818 | 27459.00000 | -0.153% | 3/7 | Overlap |
| records_array_20m / sparse | 39381.25000 | 38621.00000 | 95019.00000 | 53146.25000 | -1.930% | 0/7 | Separated gain |
| records_array_20m / scan | 40444.75000 | 39648.00000 | 90043.25000 | 53971.00000 | -1.970% | 0/7 | Separated gain |
| records_array_20m / roundtrip | 98752.00000 | 97698.50000 | 101701.00000 | 76766.50000 | -1.067% | 0/7 | Separated gain |

## 12 consumption rows — peak RSS

| Fixture / operation | Main MiB | Candidate MiB | Node MiB | Bun MiB | RSS delta MiB |
|---|---:|---:|---:|---:|---:|
| records_array_16k / sparse | 72.2031 | 72.2344 | 66.0938 | 70.6562 | +0.0312 |
| records_array_16k / scan | 204.4375 | 204.4688 | 61.9688 | 77.4062 | +0.0312 |
| records_array_16k / roundtrip | 68.7500 | 69.2344 | 65.9688 | 70.9062 | +0.4844 |
| records_array_1m / sparse | 67.6094 | 67.6406 | 125.6562 | 98.8750 | +0.0312 |
| records_array_1m / scan | 158.4844 | 158.5312 | 97.5469 | 83.2188 | +0.0469 |
| records_array_1m / roundtrip | 61.4688 | 61.4844 | 152.0469 | 93.3281 | +0.0156 |
| records_array_8m / sparse | 109.1406 | 109.1719 | 266.0000 | 187.7344 | +0.0312 |
| records_array_8m / scan | 186.7031 | 186.7500 | 230.0469 | 133.6719 | +0.0469 |
| records_array_8m / roundtrip | 129.2500 | 129.2656 | 267.4219 | 147.9844 | +0.0156 |
| records_array_20m / sparse | 255.4219 | 255.4375 | 359.9531 | 220.7188 | +0.0156 |
| records_array_20m / scan | 255.4375 | 255.4531 | 330.8125 | 223.1250 | +0.0156 |
| records_array_20m / roundtrip | 295.3438 | 295.3750 | 342.1406 | 255.4375 | +0.0312 |
