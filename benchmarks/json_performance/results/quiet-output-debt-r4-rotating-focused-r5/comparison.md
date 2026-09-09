Median process CPU in microseconds per call; RSS includes eight preloaded inputs.
Rotating inputs retain equal byte size and shape and change one value.
The same-source and selection-only controls keep those same eight inputs alive.
Selection overhead is reported without subtraction. Host admission is a separate requirement.

| Fixture | Mode | Perry µs | Node µs | Bun µs | Perry / best | Reference µs | Perry / reference |
|---|---|---:|---:|---:|---:|---:|---:|
| small_record | rotating | 0.497162 | 0.345575 | 0.254721 | 1.952 | 0.498168 | 0.998 |
| small_record | same | 0.100038 | 0.341008 | 0.246397 | 0.406 | 0.100012 | 1.000 |
| small_record | select | 0.009449 | 0.002659 | 0.003616 | 3.554 | 0.009430 | 1.002 |
| object_1k | rotating | 0.497075 | 0.540071 | 0.240689 | 2.065 | 0.497941 | 0.998 |
| object_1k | same | 0.083997 | 0.530094 | 0.228785 | 0.367 | 0.083963 | 1.000 |
| object_1k | select | 0.009428 | 0.002656 | 0.003614 | 3.550 | 0.009427 | 1.000 |
| long_string_1m | rotating | 107.804802 | 372.254067 | 70.456235 | 1.530 | 151.815647 | 0.710 |
| long_string_1m | same | 0.358730 | 376.786032 | 65.770794 | 0.005 | 0.355873 | 1.008 |
| long_string_1m | select | 0.011623 | 0.003106 | 0.003832 | 3.743 | 0.011423 | 1.018 |
| unicode_1m | rotating | 147.772222 | 438.583333 | 61.665278 | 2.396 | 186.050000 | 0.794 |
| unicode_1m | same | 0.356237 | 439.611698 | 58.904510 | 0.006 | 0.353418 | 1.008 |
| unicode_1m | select | 0.011409 | 0.003146 | 0.003877 | 3.627 | 0.011352 | 1.005 |

RSS in MiB. Each engine retains the same eight-input pool.

| Fixture | Mode | Peak: Perry / Node / Bun | After: Perry / Node / Bun | Reference peak / after |
|---|---|---:|---:|---:|
| small_record | rotating | 32.016 / 59.938 / 80.141 | 31.547 / 59.312 / 79.750 | 32.031 / 31.562 |
| small_record | same | 74.406 / 59.562 / 79.859 | 73.797 / 58.953 / 79.469 | 74.453 / 73.844 |
| small_record | select | 12.750 / 57.766 / 35.250 | 11.609 / 57.047 / 34.859 | 12.719 / 11.656 |
| object_1k | rotating | 32.344 / 61.938 / 71.422 | 31.875 / 61.328 / 71.031 | 32.391 / 31.922 |
| object_1k | same | 65.016 / 61.781 / 71.219 | 63.859 / 61.141 / 70.828 | 65.031 / 63.859 |
| object_1k | select | 12.750 / 57.812 / 35.250 | 11.609 / 57.094 / 34.859 | 12.719 / 11.656 |
| long_string_1m | rotating | 90.141 / 177.500 / 152.422 | 89.672 / 176.453 / 152.031 | 1345.828 / 1344.828 |
| long_string_1m | same | 24.578 / 200.891 / 148.578 | 23.516 / 186.234 / 148.188 | 24.547 / 23.547 |
| long_string_1m | select | 24.266 / 68.969 / 43.734 | 23.172 / 67.391 / 43.344 | 24.234 / 23.219 |
| unicode_1m | rotating | 63.266 / 160.391 / 124.891 | 62.797 / 159.781 / 124.500 | 1245.469 / 1244.469 |
| unicode_1m | same | 22.672 / 212.062 / 147.297 | 21.609 / 158.219 / 146.906 | 22.641 / 21.641 |
| unicode_1m | select | 22.375 / 68.531 / 44.641 | 21.281 / 67.828 / 44.250 | 22.328 / 21.312 |
