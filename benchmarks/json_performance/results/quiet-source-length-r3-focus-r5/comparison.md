Median process CPU in microseconds per call; RSS includes eight preloaded inputs.
Rotating inputs retain equal byte size and shape and change one value.
The same-source and selection-only controls keep those same eight inputs alive.
Selection overhead is reported without subtraction. Host admission is a separate requirement.

| Fixture | Mode | Perry µs | Node µs | Bun µs | Perry / best | Reference µs | Perry / reference |
|---|---|---:|---:|---:|---:|---:|---:|
| small_record | rotating | 0.500499 | 0.357104 | 0.255846 | 1.956 | 0.498055 | 1.005 |
| small_record | same | 0.099807 | 0.336722 | 0.245482 | 0.407 | 0.099802 | 1.000 |
| small_record | select | 0.009433 | 0.002661 | 0.003631 | 3.544 | 0.009437 | 1.000 |
| object_1k | rotating | 0.496312 | 0.540228 | 0.240822 | 2.061 | 0.496726 | 0.999 |
| object_1k | same | 0.083886 | 0.530599 | 0.228748 | 0.367 | 0.083916 | 1.000 |
| object_1k | select | 0.009428 | 0.002658 | 0.003614 | 3.548 | 0.009423 | 1.001 |
| long_string_1m | rotating | 96.101142 | 373.495106 | 70.750408 | 1.358 | 108.279772 | 0.888 |
| long_string_1m | same | 0.358539 | 376.618071 | 65.768343 | 0.005 | 0.357578 | 1.003 |
| long_string_1m | select | 0.011725 | 0.003120 | 0.003855 | 3.758 | 0.011624 | 1.009 |
| unicode_1m | rotating | 80.920908 | 438.409904 | 62.666437 | 1.291 | 147.817744 | 0.547 |
| unicode_1m | same | 0.357218 | 437.954112 | 59.091776 | 0.006 | 0.357218 | 1.000 |
| unicode_1m | select | 0.011740 | 0.003139 | 0.003857 | 3.740 | 0.011589 | 1.013 |

RSS in MiB. Each engine retains the same eight-input pool.

| Fixture | Mode | Peak: Perry / Node / Bun | After: Perry / Node / Bun | Reference peak / after |
|---|---|---:|---:|---:|
| small_record | rotating | 32.000 / 59.938 / 80.172 | 31.547 / 59.312 / 79.781 | 31.984 / 31.547 |
| small_record | same | 77.625 / 59.656 / 79.859 | 77.016 / 59.016 / 79.469 | 77.609 / 77.016 |
| small_record | select | 12.812 / 57.812 / 35.234 | 11.656 / 57.047 / 34.844 | 12.766 / 11.625 |
| object_1k | rotating | 32.328 / 61.984 / 71.453 | 31.875 / 61.359 / 71.062 | 32.312 / 31.875 |
| object_1k | same | 64.969 / 61.781 / 71.234 | 63.797 / 61.156 / 70.844 | 64.953 / 63.812 |
| object_1k | select | 12.812 / 57.750 / 35.266 | 11.656 / 57.062 / 34.875 | 12.766 / 11.625 |
| long_string_1m | rotating | 90.266 / 175.422 / 152.484 | 89.812 / 174.375 / 152.094 | 90.234 / 89.797 |
| long_string_1m | same | 24.641 / 197.578 / 134.547 | 23.562 / 176.250 / 134.156 | 24.547 / 23.500 |
| long_string_1m | select | 24.328 / 68.922 / 43.734 | 23.219 / 67.375 / 43.344 | 24.281 / 23.188 |
| unicode_1m | rotating | 62.422 / 161.234 / 147.172 | 61.969 / 160.625 / 146.781 | 62.391 / 61.953 |
| unicode_1m | same | 22.719 / 221.797 / 151.781 | 21.656 / 162.031 / 151.391 | 22.641 / 21.594 |
| unicode_1m | select | 22.438 / 68.594 / 44.625 | 21.328 / 67.891 / 44.234 | 22.391 / 21.297 |
