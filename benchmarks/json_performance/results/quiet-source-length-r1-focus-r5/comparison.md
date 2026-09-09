Median process CPU in microseconds per call; RSS includes eight preloaded inputs.
Rotating inputs retain equal byte size and shape and change one value.
The same-source and selection-only controls keep those same eight inputs alive.
Selection overhead is reported without subtraction. Host admission is a separate requirement.

| Fixture | Mode | Perry µs | Node µs | Bun µs | Perry / best | Reference µs | Perry / reference |
|---|---|---:|---:|---:|---:|---:|---:|
| small_record | rotating | 0.503749 | 0.352981 | 0.255702 | 1.970 | 0.498084 | 1.011 |
| small_record | same | 0.099874 | 0.333569 | 0.246291 | 0.406 | 0.099784 | 1.001 |
| small_record | select | 0.009438 | 0.002658 | 0.003623 | 3.551 | 0.009447 | 0.999 |
| object_1k | rotating | 0.498423 | 0.539769 | 0.240021 | 2.077 | 0.496556 | 1.004 |
| object_1k | same | 0.083969 | 0.531521 | 0.228650 | 0.367 | 0.083931 | 1.000 |
| object_1k | select | 0.009424 | 0.002659 | 0.003597 | 3.544 | 0.009432 | 0.999 |
| long_string_1m | rotating | 95.621118 | 372.118789 | 70.642857 | 1.354 | 108.041149 | 0.885 |
| long_string_1m | same | 0.359922 | 368.434456 | 66.209546 | 0.005 | 0.359268 | 1.002 |
| long_string_1m | select | 0.011602 | 0.003131 | 0.003839 | 3.706 | 0.012138 | 0.956 |
| unicode_1m | rotating | 81.300071 | 439.302210 | 62.925873 | 1.292 | 148.297933 | 0.548 |
| unicode_1m | same | 0.359381 | 438.501981 | 59.506302 | 0.006 | 0.356140 | 1.009 |
| unicode_1m | select | 0.011329 | 0.003164 | 0.003840 | 3.580 | 0.011421 | 0.992 |

RSS in MiB. Each engine retains the same eight-input pool.

| Fixture | Mode | Peak: Perry / Node / Bun | After: Perry / Node / Bun | Reference peak / after |
|---|---|---:|---:|---:|
| small_record | rotating | 32.031 / 59.938 / 80.125 | 31.578 / 59.266 / 79.734 | 32.047 / 31.609 |
| small_record | same | 76.828 / 59.609 / 79.828 | 76.219 / 58.969 / 79.438 | 76.859 / 76.266 |
| small_record | select | 12.797 / 57.781 / 35.219 | 11.641 / 57.078 / 34.828 | 12.812 / 11.656 |
| object_1k | rotating | 32.375 / 61.969 / 71.406 | 31.922 / 61.359 / 71.016 | 32.406 / 31.969 |
| object_1k | same | 65.000 / 61.781 / 71.234 | 63.859 / 61.188 / 70.844 | 65.047 / 63.891 |
| object_1k | select | 12.797 / 57.766 / 35.266 | 11.641 / 57.094 / 34.875 | 12.812 / 11.656 |
| long_string_1m | rotating | 90.250 / 177.453 / 150.484 | 89.797 / 176.453 / 150.094 | 90.297 / 89.859 |
| long_string_1m | same | 24.562 / 200.797 / 156.547 | 23.531 / 173.047 / 156.156 | 24.578 / 23.516 |
| long_string_1m | select | 24.312 / 68.969 / 43.766 | 23.219 / 67.391 / 43.375 | 24.328 / 23.219 |
| unicode_1m | rotating | 61.203 / 159.406 / 149.828 | 60.750 / 158.781 / 149.438 | 62.469 / 62.031 |
| unicode_1m | same | 22.656 / 218.141 / 153.484 | 21.625 / 143.453 / 153.094 | 22.688 / 21.625 |
| unicode_1m | select | 22.422 / 68.516 / 44.609 | 21.328 / 67.812 / 44.219 | 22.438 / 21.328 |
