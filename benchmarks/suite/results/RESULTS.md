# suite/ Node and Bun Results (generated)

Evidence: [`public-node-bun-v1.json`](../../results/public-node-bun-v1.json) · commit `defa4d6012fb480fca237ea10509da74bd41624d`
Perry: `perry 0.5.1279` · Node: `v22.23.1` · Bun: `1.3.14`
Policy: 5 measured samples per runtime and benchmark; incomplete or incorrect rows are rejected.

| Benchmark | Perry median | Node median | Bun median | Result |
|---|---:|---:|---:|---|
| 02_loop_overhead | 99 ms | 65 ms | 39 ms | loss vs both |
| 03_array_write | 1 ms | 7 ms | 5 ms | win vs both |
| 04_array_read | 24 ms | 10 ms | 13 ms | loss vs both |
| 05_fibonacci | 387 ms | 908 ms | 504 ms | win vs both |
| 06_math_intensive | 49 ms | 49 ms | 49 ms | tie |
| 07_object_create | 3 ms | 5 ms | 5 ms | win vs both |
| 08_string_concat | 1 ms | 4 ms | 1 ms | mixed |
| 09_method_calls | 79 ms | 10 ms | 8 ms | loss vs both |
| 10_nested_loops | 42 ms | 17 ms | 18 ms | loss vs both |
| 11_prime_sieve | 107 ms | 6 ms | 5 ms | loss vs both |
| 12_binary_trees | 4 ms | 6 ms | 6 ms | win vs both |
| 13_factorial | 94 ms | 95 ms | 95 ms | win vs both |
| 14_closure | 46 ms | 49 ms | 49 ms | win vs both |
| 15_mandelbrot | 22 ms | 24 ms | 28 ms | win vs both |
| 16_matrix_multiply | 637 ms | 33 ms | 33 ms | loss vs both |
| bench_gc_pressure | 28 ms | 15 ms | 19 ms | loss vs both |
| bench_json_roundtrip | 212 ms | 390 ms | 219 ms | win vs both |
| bench_object_property | 126 ms | 16 ms | 9 ms | loss vs both |
| bench_int_arithmetic | 357 ms | 94 ms | 39 ms | loss vs both |
| bench_buffer_readwrite | 97 ms | 98 ms | 194 ms | win vs both |
| bench_array_grow | 21 ms | 14 ms | 9 ms | loss vs both |
| bench_string_heavy | 56 ms | 41 ms | 29 ms | loss vs both |
| bench_numeric_array_numeric | 69 ms | 5 ms | 4 ms | loss vs both |
| bench_numeric_array_downgrade | 19 ms | 6 ms | 5 ms | loss vs both |

## Summary

- Wins versus both peers: **9**
- Losses versus both peers: **13**
- Mixed or tied rows: **2**

> Historical note: the former v0.5.908 single-run commentary is archived in Git history and is not current evidence.
