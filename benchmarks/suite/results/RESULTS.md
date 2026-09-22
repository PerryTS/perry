# suite/ Node and Bun Results (generated)

Evidence: [`public-node-bun-v1.json`](../../results/public-node-bun-v1.json) · commit `689b1ffbab1b9cc1825ce69d6cf7cbe50fabb168`
Perry: `perry 0.5.1636` · Node: `v22.23.1` · Bun: `1.3.14`
Policy: 5 measured samples per runtime and benchmark; incomplete or incorrect rows are rejected.

| Benchmark | Perry median | Node median | Bun median | Result |
|---|---:|---:|---:|---|
| 02_loop_overhead | 125 ms | 126 ms | 126 ms | win vs both |
| 03_array_write | 1 ms | 6 ms | 5 ms | win vs both |
| 04_array_read | 9 ms | 11 ms | 14 ms | win vs both |
| 05_fibonacci | 338 ms | 913 ms | 504 ms | win vs both |
| 06_math_intensive | 48 ms | 49 ms | 49 ms | win vs both |
| 07_object_create | 8 ms | 5 ms | 6 ms | loss vs both |
| 08_string_concat | 5 ms | 31 ms | 6 ms | win vs both |
| 09_method_calls | 34 ms | 11 ms | 8 ms | loss vs both |
| 10_nested_loops | 17 ms | 18 ms | 19 ms | win vs both |
| 11_prime_sieve | 6 ms | 5 ms | 5 ms | loss vs both |
| 12_binary_trees | 13 ms | 6 ms | 6 ms | loss vs both |
| 13_factorial | 94 ms | 95 ms | 95 ms | win vs both |
| 14_closure | 47 ms | 49 ms | 49 ms | win vs both |
| 15_mandelbrot | 25 ms | 24 ms | 29 ms | mixed |
| 16_matrix_multiply | 18 ms | 32 ms | 33 ms | win vs both |
| bench_gc_pressure | 13 ms | 15 ms | 20 ms | win vs both |
| bench_json_roundtrip | 142 ms | 387 ms | 220 ms | win vs both |
| bench_object_property | 9 ms | 16 ms | 10 ms | win vs both |
| bench_int_arithmetic | 52 ms | 93 ms | 38 ms | mixed |
| bench_buffer_readwrite | 32 ms | 97 ms | 193 ms | win vs both |
| bench_array_grow | 8 ms | 13 ms | 9 ms | win vs both |
| bench_string_heavy | 36 ms | 42 ms | 28 ms | mixed |
| bench_numeric_array_numeric | 7 ms | 5 ms | 4 ms | loss vs both |
| bench_numeric_array_downgrade | 4 ms | 6 ms | 5 ms | win vs both |

## Summary

- Wins versus both peers: **16**
- Losses versus both peers: **5**
- Mixed or tied rows: **3**

> Historical note: the former v0.5.908 single-run commentary is archived in Git history and is not current evidence.
