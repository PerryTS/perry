# suite/ Node and Bun Results (generated)

Evidence: [`public-node-bun-v1.json`](../../results/public-node-bun-v1.json) · commit `38ff7eccc2aa0b59629bed548daf8ac56bd9fb03`
Perry: `perry 0.5.1355` · Node: `v22.23.1` · Bun: `1.3.14`
Policy: 5 measured samples per runtime and benchmark; incomplete or incorrect rows are rejected.

| Benchmark | Perry median | Node median | Bun median | Result |
|---|---:|---:|---:|---|
| 02_loop_overhead | 94 ms | 64 ms | 39 ms | loss vs both |
| 03_array_write | 1 ms | 7 ms | 5 ms | win vs both |
| 04_array_read | 23 ms | 11 ms | 13 ms | loss vs both |
| 05_fibonacci | 390 ms | 913 ms | 505 ms | win vs both |
| 06_math_intensive | 49 ms | 49 ms | 48 ms | mixed |
| 07_object_create | 3 ms | 5 ms | 5 ms | win vs both |
| 08_string_concat | 2 ms | 4 ms | 1 ms | mixed |
| 09_method_calls | 9 ms | 10 ms | 8 ms | mixed |
| 10_nested_loops | 39 ms | 17 ms | 19 ms | loss vs both |
| 11_prime_sieve | 28 ms | 6 ms | 5 ms | loss vs both |
| 12_binary_trees | 4 ms | 6 ms | 6 ms | win vs both |
| 13_factorial | 94 ms | 95 ms | 95 ms | win vs both |
| 14_closure | 47 ms | 49 ms | 49 ms | win vs both |
| 15_mandelbrot | 22 ms | 24 ms | 28 ms | win vs both |
| 16_matrix_multiply | 85 ms | 33 ms | 33 ms | loss vs both |
| bench_gc_pressure | 21 ms | 15 ms | 20 ms | loss vs both |
| bench_json_roundtrip | 184 ms | 384 ms | 219 ms | win vs both |
| bench_object_property | 129 ms | 15 ms | 7 ms | loss vs both |
| bench_int_arithmetic | 367 ms | 94 ms | 39 ms | loss vs both |
| bench_buffer_readwrite | 94 ms | 96 ms | 193 ms | win vs both |
| bench_array_grow | 8 ms | 13 ms | 9 ms | win vs both |
| bench_string_heavy | 50 ms | 42 ms | 29 ms | loss vs both |
| bench_numeric_array_numeric | 68 ms | 5 ms | 4 ms | loss vs both |
| bench_numeric_array_downgrade | 17 ms | 6 ms | 5 ms | loss vs both |

## Summary

- Wins versus both peers: **10**
- Losses versus both peers: **11**
- Mixed or tied rows: **3**

> Historical note: the former v0.5.908 single-run commentary is archived in Git history and is not current evidence.
