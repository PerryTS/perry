Median process CPU in microseconds per call; RSS includes eight preloaded inputs.
Rotating inputs retain equal byte size and shape and change one value.
The same-source and selection-only controls keep those same eight inputs alive.
Selection overhead is reported without subtraction. Host admission is a separate requirement.

| Fixture | Mode | Perry µs | Node µs | Bun µs | Perry / best | Reference µs | Perry / reference |
|---|---|---:|---:|---:|---:|---:|---:|
| small_record | rotating | 0.497636 | 0.332631 | 0.257086 | 1.936 | 0.499053 | 0.997 |
| small_record | same | 0.099282 | 0.334542 | 0.245240 | 0.405 | 0.099898 | 0.994 |
| small_record | select | 0.009440 | 0.002657 | 0.003606 | 3.554 | 0.009433 | 1.001 |
| object_1k | rotating | 0.483048 | 0.540683 | 0.240620 | 2.008 | 0.499924 | 0.966 |
| object_1k | same | 0.083484 | 0.531066 | 0.229102 | 0.364 | 0.084180 | 0.992 |
| object_1k | select | 0.009439 | 0.002657 | 0.003619 | 3.553 | 0.009429 | 1.001 |
| long_string_1m | rotating | 100.055466 | 372.643087 | 69.530547 | 1.439 | 108.215434 | 0.925 |
| long_string_1m | same | 0.359107 | 376.963766 | 65.782918 | 0.005 | 0.359431 | 0.999 |
| long_string_1m | select | 0.011769 | 0.003113 | 0.003842 | 3.780 | 0.011456 | 1.027 |
| unicode_1m | rotating | 141.572546 | 439.354908 | 63.518492 | 2.229 | 148.320057 | 0.955 |
| unicode_1m | same | 0.355023 | 436.863068 | 59.150518 | 0.006 | 0.356811 | 0.995 |
| unicode_1m | select | 0.011372 | 0.003129 | 0.003854 | 3.634 | 0.011439 | 0.994 |
| wide_1m | rotating | 2685.640625 | 5448.625000 | 4164.000000 | 0.645 | 2683.031250 | 1.001 |
| wide_1m | same | 2673.390625 | 5422.593750 | 4156.406250 | 0.643 | 2672.968750 | 1.000 |
| wide_1m | select | 0.011354 | 0.003136 | 0.003833 | 3.621 | 0.011283 | 1.006 |
| heterogeneous_1m | rotating | 1427.670455 | 3924.704545 | 2959.022727 | 0.482 | 1428.909091 | 0.999 |
| heterogeneous_1m | same | 1424.647727 | 3833.943182 | 2968.772727 | 0.480 | 1427.625000 | 0.998 |
| heterogeneous_1m | select | 0.011564 | 0.003112 | 0.003866 | 3.715 | 0.011452 | 1.010 |

RSS in MiB. Each engine retains the same eight-input pool.

| Fixture | Mode | Peak: Perry / Node / Bun | After: Perry / Node / Bun | Reference peak / after |
|---|---|---:|---:|---:|
| small_record | rotating | 31.938 / 59.859 / 80.172 | 31.422 / 59.250 / 79.781 | 31.984 / 31.547 |
| small_record | same | 72.781 / 59.594 / 79.828 | 72.125 / 58.969 / 79.438 | 72.812 / 72.234 |
| small_record | select | 12.875 / 57.812 / 35.234 | 11.766 / 57.047 / 34.844 | 12.766 / 11.625 |
| object_1k | rotating | 32.297 / 61.938 / 71.438 | 31.781 / 61.344 / 71.047 | 32.344 / 31.906 |
| object_1k | same | 64.938 / 61.750 / 71.234 | 63.703 / 61.109 / 70.844 | 64.953 / 63.812 |
| object_1k | select | 12.875 / 57.828 / 35.250 | 11.766 / 57.094 / 34.859 | 12.766 / 11.625 |
| long_string_1m | rotating | 90.219 / 175.484 / 123.484 | 89.703 / 174.438 / 123.094 | 90.219 / 89.781 |
| long_string_1m | same | 24.641 / 208.109 / 145.500 | 23.641 / 188.016 / 145.109 | 24.547 / 23.500 |
| long_string_1m | select | 24.375 / 68.969 / 43.719 | 23.328 / 67.391 / 43.328 | 24.281 / 23.188 |
| unicode_1m | rotating | 62.391 / 159.406 / 151.641 | 61.875 / 158.828 / 151.250 | 62.375 / 61.938 |
| unicode_1m | same | 22.750 / 221.547 / 147.328 | 21.750 / 151.078 / 146.938 | 22.656 / 21.609 |
| unicode_1m | select | 22.484 / 68.516 / 44.641 | 21.438 / 67.812 / 44.250 | 22.391 / 21.297 |
| wide_1m | rotating | 252.266 / 145.938 / 97.766 | 251.766 / 145.359 / 97.375 | 252.281 / 251.844 |
| wide_1m | same | 252.266 / 145.875 / 97.750 | 251.766 / 145.312 / 97.359 | 252.266 / 251.828 |
| wide_1m | select | 23.422 / 68.281 / 43.141 | 22.375 / 67.516 / 42.750 | 23.328 / 22.234 |
| heterogeneous_1m | rotating | 85.828 / 97.219 / 92.266 | 85.344 / 95.922 / 91.875 | 85.844 / 85.438 |
| heterogeneous_1m | same | 85.828 / 97.203 / 92.047 | 85.344 / 95.891 / 91.656 | 85.844 / 85.438 |
| heterogeneous_1m | select | 22.797 / 67.688 / 42.625 | 21.750 / 66.953 / 42.234 | 22.703 / 21.609 |
