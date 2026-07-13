# suite/ Node and Bun Results (generated)

Evidence: [`public-node-bun-v1.json`](../../results/public-node-bun-v1.json) · commit `36cf04e0147c68a55f362689ab6c56e2b39a3c67`
Perry: `perry 0.5.1258` · Node: `v22.23.1` · Bun: `1.3.14`
Policy: 5 measured samples per runtime and benchmark; incomplete or incorrect rows are rejected.

| Benchmark | Perry median | Node median | Bun median | Result |
|---|---:|---:|---:|---|
| 02_loop_overhead | 97 ms | 66 ms | 41 ms | loss vs both |
| 03_array_write | 5 ms | 8 ms | 6 ms | win vs both |
| 04_array_read | 97 ms | 12 ms | 17 ms | loss vs both |
| 05_fibonacci | 310 ms | 948 ms | 523 ms | win vs both |
| 06_math_intensive | 51 ms | 51 ms | 51 ms | tie |
| 07_object_create | 4 ms | 5 ms | 6 ms | win vs both |
| 08_string_concat | 3 ms | 5 ms | 1 ms | mixed |
| 09_method_calls | 82 ms | 11 ms | 8 ms | loss vs both |
| 10_nested_loops | 164 ms | 19 ms | 20 ms | loss vs both |
| 11_prime_sieve | 254 ms | 7 ms | 5 ms | loss vs both |
| 12_binary_trees | 6 ms | 7 ms | 8 ms | win vs both |
| 13_factorial | 1555 ms | 99 ms | 99 ms | loss vs both |
| 14_closure | 49 ms | 51 ms | 51 ms | win vs both |
| 15_mandelbrot | 23 ms | 26 ms | 30 ms | win vs both |
| 16_matrix_multiply | 2311 ms | 35 ms | 35 ms | loss vs both |
| bench_gc_pressure | 20 ms | 18 ms | 29 ms | mixed |
| bench_json_roundtrip | 447 ms | 421 ms | 238 ms | loss vs both |
| bench_object_property | 275 ms | 16 ms | 11 ms | loss vs both |
| bench_int_arithmetic | 577 ms | 101 ms | 41 ms | loss vs both |
| bench_buffer_readwrite | 96 ms | 100 ms | 105 ms | win vs both |
| bench_array_grow | 150 ms | 21 ms | 11 ms | loss vs both |
| bench_string_heavy | 74 ms | 52 ms | 31 ms | loss vs both |
| bench_numeric_array_numeric | 154 ms | 6 ms | 6 ms | loss vs both |
| bench_numeric_array_downgrade | 11028 ms | 7 ms | 6 ms | loss vs both |

## Summary

- Wins versus both peers: **7**
- Losses versus both peers: **14**
- Mixed or tied rows: **3**

> Historical note: the former v0.5.908 single-run commentary is archived in Git history and is not current evidence.
