Median process CPU in microseconds per call; RSS includes eight preloaded inputs.
Rotating inputs retain equal byte size and shape and change one value.
The same-source and selection-only controls keep those same eight inputs alive.
Selection overhead is reported without subtraction. Host admission is a separate requirement.

| Fixture | Mode | Perry µs | Node µs | Bun µs | Perry / best | Reference µs | Perry / reference |
|---|---|---:|---:|---:|---:|---:|---:|
| small_record | rotating | 0.500501 | 0.340577 | 0.255562 | 1.958 | 0.498057 | 1.005 |
| small_record | same | 0.099841 | 0.350136 | 0.246472 | 0.405 | 0.099768 | 1.001 |
| small_record | select | 0.009428 | 0.002655 | 0.003612 | 3.551 | 0.009470 | 0.996 |
| object_1k | rotating | 0.496718 | 0.540328 | 0.239942 | 2.070 | 0.497891 | 0.998 |
| object_1k | same | 0.083993 | 0.531057 | 0.228700 | 0.367 | 0.084040 | 0.999 |
| object_1k | select | 0.009429 | 0.002658 | 0.003605 | 3.547 | 0.009432 | 1.000 |
| long_string_1m | rotating | 95.981673 | 372.566534 | 70.133068 | 1.369 | 108.004781 | 0.889 |
| long_string_1m | same | 0.358175 | 376.231288 | 66.008031 | 0.005 | 0.358497 | 0.999 |
| long_string_1m | select | 0.012017 | 0.003129 | 0.003848 | 3.840 | 0.011399 | 1.054 |
| unicode_1m | rotating | 81.156073 | 439.552966 | 63.521893 | 1.278 | 148.399718 | 0.547 |
| unicode_1m | same | 0.356125 | 434.659544 | 59.514245 | 0.006 | 0.356125 | 1.000 |
| unicode_1m | select | 0.011151 | 0.003125 | 0.003853 | 3.568 | 0.011394 | 0.979 |

RSS in MiB. Each engine retains the same eight-input pool.

| Fixture | Mode | Peak: Perry / Node / Bun | After: Perry / Node / Bun | Reference peak / after |
|---|---|---:|---:|---:|
| small_record | rotating | 32.000 / 59.906 / 80.172 | 31.547 / 59.266 / 79.781 | 32.047 / 31.609 |
| small_record | same | 75.188 / 59.594 / 79.844 | 74.594 / 58.938 / 79.453 | 75.219 / 74.641 |
| small_record | select | 12.766 / 57.797 / 35.266 | 11.656 / 57.062 / 34.875 | 12.828 / 11.672 |
| object_1k | rotating | 32.375 / 62.016 / 71.422 | 31.922 / 61.406 / 71.031 | 32.406 / 31.969 |
| object_1k | same | 64.984 / 61.734 / 71.203 | 63.844 / 61.078 / 70.812 | 65.016 / 63.859 |
| object_1k | select | 12.766 / 57.859 / 35.281 | 11.656 / 57.125 / 34.891 | 12.828 / 11.672 |
| long_string_1m | rotating | 90.281 / 176.422 / 145.453 | 89.828 / 175.312 / 145.062 | 90.281 / 89.844 |
| long_string_1m | same | 24.531 / 203.438 / 150.484 | 23.531 / 176.109 / 150.094 | 24.578 / 23.516 |
| long_string_1m | select | 24.281 / 68.984 / 43.750 | 23.234 / 67.438 / 43.359 | 24.312 / 23.203 |
| unicode_1m | rotating | 62.422 / 160.266 / 153.469 | 61.969 / 159.703 / 153.078 | 62.438 / 62.000 |
| unicode_1m | same | 22.656 / 201.109 / 153.469 | 21.656 / 200.219 / 153.078 | 22.688 / 21.625 |
| unicode_1m | select | 22.391 / 68.578 / 44.625 | 21.344 / 67.859 / 44.234 | 22.422 / 21.312 |
