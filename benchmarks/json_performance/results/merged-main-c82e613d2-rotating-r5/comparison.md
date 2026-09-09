Median process CPU in microseconds per call; RSS includes eight preloaded inputs.
Rotating inputs retain equal byte size and shape and change one value.
The same-source and selection-only controls keep those same eight inputs alive.
Selection overhead is reported without subtraction. Host admission is a separate requirement.

| Fixture | Mode | Perry µs | Node µs | Bun µs | Perry / best |
|---|---|---:|---:|---:|---:|
| null | rotating | 0.016363 | 0.028782 | 0.020799 | 0.787 |
| null | same | 0.012410 | 0.028394 | 0.020561 | 0.604 |
| null | select | 0.009803 | 0.002898 | 0.004043 | 3.383 |
| string_a | rotating | 0.022077 | 0.033858 | 0.025091 | 0.880 |
| string_a | same | 0.018155 | 0.033772 | 0.024586 | 0.738 |
| string_a | select | 0.009987 | 0.002971 | 0.004136 | 3.361 |
| empty_object | rotating | 0.031933 | 0.048088 | 0.027345 | 1.168 |
| empty_object | same | 0.026822 | 0.047286 | 0.026926 | 0.996 |
| empty_object | select | 0.009893 | 0.002997 | 0.004036 | 3.302 |
| tiny_object | rotating | 0.049712 | 0.085646 | 0.049443 | 1.005 |
| tiny_object | same | 0.043532 | 0.084501 | 0.048718 | 0.894 |
| tiny_object | select | 0.010025 | 0.003051 | 0.003991 | 3.285 |
| small_record | rotating | 0.511205 | 0.373772 | 0.267734 | 1.909 |
| small_record | same | 0.105083 | 0.359005 | 0.255905 | 0.411 |
| small_record | select | 0.009729 | 0.002885 | 0.003902 | 3.372 |
| object_1k | rotating | 0.515262 | 0.539946 | 0.249892 | 2.062 |
| object_1k | same | 0.089306 | 0.532152 | 0.237362 | 0.376 |
| object_1k | select | 0.009893 | 0.002890 | 0.003893 | 3.423 |
| records_array_16k | rotating | 20.139726 | 43.976311 | 35.368381 | 0.569 |
| records_array_16k | same | 19.845990 | 41.899230 | 35.416888 | 0.560 |
| records_array_16k | select | 0.009824 | 0.004343 | 0.004649 | 2.262 |
| records_array_1m | rotating | 1315.180328 | 2985.278689 | 2360.040984 | 0.557 |
| records_array_1m | same | 1325.436508 | 2940.523810 | 2355.928571 | 0.563 |
| records_array_1m | select | 0.012131 | 0.003458 | 0.004243 | 3.508 |
| records_object_1m | rotating | 2162.600000 | 3070.553846 | 2383.646154 | 0.907 |
| records_object_1m | same | 2184.796875 | 3211.390625 | 2446.062500 | 0.893 |
| records_object_1m | select | 0.013260 | 0.004223 | 0.005638 | 3.140 |
| records_array_8m | rotating | 10445.642857 | 32889.928571 | 21083.000000 | 0.495 |
| records_array_8m | same | 10806.785714 | 34902.214286 | 21586.357143 | 0.501 |
| records_array_8m | select | 0.012679 | 0.003556 | 0.004247 | 3.566 |
| records_object_8m | rotating | 17219.000000 | 37690.875000 | 21855.625000 | 0.788 |
| records_object_8m | same | 17182.000000 | 35383.125000 | 22621.750000 | 0.760 |
| records_object_8m | select | 0.012986 | 0.003674 | 0.004446 | 3.534 |
| records_array_20m | rotating | 45773.000000 | 78839.000000 | 47574.500000 | 0.962 |
| records_array_20m | same | 45308.375000 | 80384.750000 | 46973.375000 | 0.965 |
| records_array_20m | select | 0.013728 | 0.015019 | 0.010750 | 1.277 |
| records_object_20m | rotating | 46042.250000 | 80116.125000 | 47119.625000 | 0.977 |
| records_object_20m | same | 45974.625000 | 84627.250000 | 47954.750000 | 0.959 |
| records_object_20m | select | 0.014723 | 0.005845 | 0.004327 | 3.402 |
| numbers_1m | rotating | 1742.243590 | 3385.230769 | 3391.756410 | 0.515 |
| numbers_1m | same | 1705.230769 | 3316.987179 | 3369.076923 | 0.514 |
| numbers_1m | select | 0.012684 | 0.003438 | 0.004325 | 3.690 |
| long_string_1m | rotating | 172.504697 | 393.083689 | 76.490179 | 2.255 |
| long_string_1m | same | 0.373507 | 382.699602 | 73.452769 | 0.005 |
| long_string_1m | select | 0.013274 | 0.003482 | 0.004378 | 3.812 |
| escaped_1m | rotating | 1081.226027 | 1816.705479 | 2188.664384 | 0.595 |
| escaped_1m | same | 1073.128378 | 1812.783784 | 2176.398649 | 0.592 |
| escaped_1m | select | 0.012488 | 0.003429 | 0.004131 | 3.641 |
| unicode_1m | rotating | 204.999245 | 465.434290 | 69.387462 | 2.954 |
| unicode_1m | same | 0.376453 | 484.150194 | 76.905523 | 0.005 |
| unicode_1m | select | 0.012446 | 0.003459 | 0.004177 | 3.599 |
| wide_1m | rotating | 2819.566667 | 7151.683333 | 5044.466667 | 0.559 |
| wide_1m | same | 2981.606557 | 9843.147541 | 6305.213115 | 0.473 |
| wide_1m | select | 0.012508 | 0.005076 | 0.006110 | 2.464 |
| heterogeneous_1m | rotating | 1573.800000 | 4823.237500 | 3693.137500 | 0.426 |
| heterogeneous_1m | same | 1557.439024 | 4599.060976 | 3401.280488 | 0.458 |
| heterogeneous_1m | select | 0.013164 | 0.003612 | 0.004465 | 3.645 |
