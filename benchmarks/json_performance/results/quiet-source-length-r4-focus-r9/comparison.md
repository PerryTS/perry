Median process CPU in microseconds per call; RSS includes eight preloaded inputs.
Rotating inputs retain equal byte size and shape and change one value.
The same-source and selection-only controls keep those same eight inputs alive.
Selection overhead is reported without subtraction. Host admission is a separate requirement.

| Fixture | Mode | Perry µs | Node µs | Bun µs | Perry / best | Reference µs | Perry / reference |
|---|---|---:|---:|---:|---:|---:|---:|
| small_record | rotating | 0.500487 | 0.343732 | 0.257312 | 1.945 | 0.499364 | 1.002 |
| small_record | same | 0.099989 | 0.347565 | 0.246611 | 0.405 | 0.099996 | 1.000 |
| small_record | select | 0.009432 | 0.002661 | 0.003619 | 3.544 | 0.009433 | 1.000 |
| object_1k | rotating | 0.495775 | 0.541344 | 0.240213 | 2.064 | 0.497295 | 0.997 |
| object_1k | same | 0.084070 | 0.531058 | 0.229036 | 0.367 | 0.084093 | 1.000 |
| object_1k | select | 0.009437 | 0.002653 | 0.003622 | 3.557 | 0.009435 | 1.000 |
| long_string_1m | rotating | 96.438941 | 373.351836 | 69.196413 | 1.394 | 108.670367 | 0.887 |
| long_string_1m | same | 0.357440 | 376.116480 | 65.736640 | 0.005 | 0.359680 | 0.994 |
| long_string_1m | select | 0.012237 | 0.003109 | 0.003863 | 3.936 | 0.011966 | 1.023 |
| unicode_1m | rotating | 81.329763 | 439.146901 | 63.103290 | 1.289 | 148.643458 | 0.547 |
| unicode_1m | same | 0.355641 | 438.095305 | 59.155922 | 0.006 | 0.356342 | 0.998 |
| unicode_1m | select | 0.011817 | 0.003145 | 0.003873 | 3.757 | 0.011470 | 1.030 |

RSS in MiB. Each engine retains the same eight-input pool.

| Fixture | Mode | Peak: Perry / Node / Bun | After: Perry / Node / Bun | Reference peak / after |
|---|---|---:|---:|---:|
| small_record | rotating | 32.016 / 59.844 / 80.172 | 31.562 / 59.234 / 79.781 | 31.984 / 31.547 |
| small_record | same | 70.547 / 59.594 / 79.812 | 69.953 / 58.969 / 79.422 | 70.516 / 69.938 |
| small_record | select | 12.844 / 57.859 / 35.234 | 11.688 / 57.125 / 34.844 | 12.766 / 11.625 |
| object_1k | rotating | 32.344 / 62.000 / 71.406 | 31.891 / 61.406 / 71.016 | 32.328 / 31.891 |
| object_1k | same | 65.000 / 61.719 / 71.219 | 63.812 / 61.062 / 70.828 | 64.953 / 63.812 |
| object_1k | select | 12.844 / 57.750 / 35.266 | 11.688 / 57.062 / 34.875 | 12.766 / 11.625 |
| long_string_1m | rotating | 90.250 / 173.422 / 123.453 | 89.797 / 172.359 / 123.062 | 90.203 / 89.766 |
| long_string_1m | same | 24.656 / 203.125 / 142.562 | 23.578 / 178.266 / 142.172 | 24.547 / 23.484 |
| long_string_1m | select | 24.359 / 68.922 / 43.750 | 23.250 / 67.344 / 43.359 | 24.281 / 23.188 |
| unicode_1m | rotating | 61.203 / 156.641 / 157.000 | 60.750 / 156.078 / 156.609 | 61.156 / 60.719 |
| unicode_1m | same | 22.734 / 217.875 / 157.062 | 21.672 / 171.609 / 156.672 | 22.656 / 21.609 |
| unicode_1m | select | 22.469 / 68.547 / 44.641 | 21.359 / 67.844 / 44.250 | 22.375 / 21.281 |
