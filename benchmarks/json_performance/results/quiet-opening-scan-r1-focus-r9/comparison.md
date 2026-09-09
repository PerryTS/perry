Median process CPU in microseconds per call; RSS includes eight preloaded inputs.
Rotating inputs retain equal byte size and shape and change one value.
The same-source and selection-only controls keep those same eight inputs alive.
Selection overhead is reported without subtraction. Host admission is a separate requirement.

| Fixture | Mode | Perry µs | Node µs | Bun µs | Perry / best | Reference µs | Perry / reference |
|---|---|---:|---:|---:|---:|---:|---:|
| small_record | rotating | 0.497019 | 0.348770 | 0.254688 | 1.951 | 0.498251 | 0.998 |
| small_record | same | 0.099893 | 0.345451 | 0.245259 | 0.407 | 0.099831 | 1.001 |
| small_record | select | 0.009437 | 0.002661 | 0.003615 | 3.546 | 0.009432 | 1.001 |
| object_1k | rotating | 0.480048 | 0.541670 | 0.240352 | 1.997 | 0.496794 | 0.966 |
| object_1k | same | 0.084151 | 0.530947 | 0.228936 | 0.368 | 0.084145 | 1.000 |
| object_1k | select | 0.009465 | 0.002655 | 0.003623 | 3.564 | 0.009432 | 1.003 |
| long_string_1m | rotating | 99.856115 | 372.404476 | 70.655476 | 1.413 | 108.099121 | 0.924 |
| long_string_1m | same | 0.359885 | 376.237684 | 66.061420 | 0.005 | 0.358925 | 1.003 |
| long_string_1m | select | 0.011609 | 0.003113 | 0.003879 | 3.729 | 0.011923 | 0.974 |
| unicode_1m | rotating | 141.155556 | 438.534028 | 62.728472 | 2.250 | 147.871528 | 0.955 |
| unicode_1m | same | 0.356387 | 438.154552 | 59.019054 | 0.006 | 0.357445 | 0.997 |
| unicode_1m | select | 0.011544 | 0.003136 | 0.003838 | 3.681 | 0.011256 | 1.026 |

RSS in MiB. Each engine retains the same eight-input pool.

| Fixture | Mode | Peak: Perry / Node / Bun | After: Perry / Node / Bun | Reference peak / after |
|---|---|---:|---:|---:|
| small_record | rotating | 31.969 / 59.875 / 80.172 | 31.516 / 59.234 / 79.781 | 31.984 / 31.547 |
| small_record | same | 74.375 / 59.625 / 79.844 | 73.781 / 58.953 / 79.453 | 74.375 / 73.797 |
| small_record | select | 12.797 / 57.781 / 35.234 | 11.672 / 57.062 / 34.844 | 12.766 / 11.625 |
| object_1k | rotating | 32.328 / 61.938 / 71.406 | 31.875 / 61.328 / 71.016 | 32.312 / 31.875 |
| object_1k | same | 64.969 / 61.766 / 71.234 | 63.781 / 61.156 / 70.844 | 64.953 / 63.812 |
| object_1k | select | 12.797 / 57.828 / 35.281 | 11.672 / 57.094 / 34.891 | 12.766 / 11.625 |
| long_string_1m | rotating | 90.219 / 175.453 / 152.469 | 89.766 / 174.375 / 152.078 | 90.219 / 89.781 |
| long_string_1m | same | 24.594 / 226.125 / 156.562 | 23.547 / 176.422 / 156.172 | 24.562 / 23.500 |
| long_string_1m | select | 24.312 / 68.984 / 43.750 | 23.234 / 67.391 / 43.359 | 24.281 / 23.188 |
| unicode_1m | rotating | 62.375 / 160.344 / 151.719 | 61.922 / 159.750 / 151.328 | 62.406 / 61.969 |
| unicode_1m | same | 22.703 / 223.422 / 151.641 | 21.672 / 158.219 / 151.250 | 22.656 / 21.609 |
| unicode_1m | select | 22.422 / 68.562 / 44.641 | 21.344 / 67.859 / 44.250 | 22.391 / 21.297 |
