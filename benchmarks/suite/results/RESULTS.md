# suite/ Node and Bun Results (generated)

Evidence: [`public-node-bun-v1.json`](../../results/public-node-bun-v1.json) · commit `2ba59501b9ea8c37c578dd9128e48b667e42bc1d`
Perry: `perry 0.5.1335` · Node: `v22.23.1` · Bun: `1.3.14`
Policy: 5 measured samples per runtime and benchmark; incomplete or incorrect rows are rejected.

| Benchmark | Perry median | Node median | Bun median | Result |
|---|---:|---:|---:|---|
| 02_loop_overhead | 95 ms | 65 ms | 40 ms | loss vs both |
| 03_array_write | 1 ms | 7 ms | 5 ms | win vs both |
| 04_array_read | 23 ms | 11 ms | 14 ms | loss vs both |
| 05_fibonacci | 401 ms | 932 ms | 515 ms | win vs both |
| 06_math_intensive | 50 ms | 50 ms | 50 ms | tie |
| 07_object_create | 2 ms | 5 ms | 6 ms | win vs both |
| 08_string_concat | 1 ms | 4 ms | 1 ms | mixed |
| 09_method_calls | 10 ms | 11 ms | 8 ms | mixed |
| 10_nested_loops | 41 ms | 18 ms | 19 ms | loss vs both |
| 11_prime_sieve | 30 ms | 6 ms | 6 ms | loss vs both |
| 12_binary_trees | 4 ms | 6 ms | 6 ms | win vs both |
| 13_factorial | 96 ms | 97 ms | 96 ms | mixed |
| 14_closure | 48 ms | 50 ms | 50 ms | win vs both |
| 15_mandelbrot | 22 ms | 26 ms | 30 ms | win vs both |
| 16_matrix_multiply | 87 ms | 34 ms | 34 ms | loss vs both |
| bench_gc_pressure | 22 ms | 16 ms | 20 ms | loss vs both |
| bench_json_roundtrip | 192 ms | 401 ms | 227 ms | win vs both |
| bench_object_property | 122 ms | 16 ms | 10 ms | loss vs both |
| bench_int_arithmetic | 361 ms | 96 ms | 40 ms | loss vs both |
| bench_buffer_readwrite | 94 ms | 99 ms | 105 ms | win vs both |
| bench_array_grow | 18 ms | 14 ms | 9 ms | loss vs both |
| bench_string_heavy | 51 ms | 43 ms | 29 ms | loss vs both |
| bench_numeric_array_numeric | 70 ms | 5 ms | 4 ms | loss vs both |
| bench_numeric_array_downgrade | 21 ms | 6 ms | 6 ms | loss vs both |

## Summary

- Wins versus both peers: **8**
- Losses versus both peers: **12**
- Mixed or tied rows: **4**

> Historical note: the former v0.5.908 single-run commentary is archived in Git history and is not current evidence.
