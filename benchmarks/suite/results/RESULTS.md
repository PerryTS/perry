# suite/ Node and Bun Results (generated)

Evidence: [`public-node-bun-v1.json`](../../results/public-node-bun-v1.json) · commit `827a92bad5a589e94e1630994185693ada903fe2`
Perry: `perry 0.5.1519` · Node: `v22.23.1` · Bun: `1.3.14`
Policy: 5 measured samples per runtime and benchmark; incomplete or incorrect rows are rejected.

| Benchmark | Perry median | Node median | Bun median | Result |
|---|---:|---:|---:|---|
| 02_loop_overhead | 74 ms | 75 ms | 74 ms | mixed |
| 03_array_write | 4 ms | 5 ms | 5 ms | win vs both |
| 04_array_read | 5 ms | 7 ms | 9 ms | win vs both |
| 05_fibonacci | 315 ms | 698 ms | 426 ms | win vs both |
| 06_math_intensive | 44 ms | 43 ms | 47 ms | mixed |
| 07_object_create | 4 ms | 4 ms | 7 ms | mixed |
| 08_string_concat | 5 ms | 37 ms | 12 ms | win vs both |
| 09_method_calls | 6 ms | 9 ms | 4 ms | mixed |
| 10_nested_loops | 25 ms | 12 ms | 12 ms | loss vs both |
| 11_prime_sieve | 4 ms | 4 ms | 5 ms | mixed |
| 12_binary_trees | 6 ms | 5 ms | 9 ms | mixed |
| 13_factorial | 59 ms | 95 ms | 61 ms | win vs both |
| 14_closure | 29 ms | 30 ms | 29 ms | mixed |
| 15_mandelbrot | 15 ms | 17 ms | 16 ms | win vs both |
| 16_matrix_multiply | 22 ms | 24 ms | 22 ms | mixed |
| bench_gc_pressure | 12 ms | 15 ms | 28 ms | win vs both |
| bench_json_roundtrip | 184 ms | 380 ms | 185 ms | win vs both |
| bench_object_property | 26 ms | 13 ms | 17 ms | loss vs both |
| bench_int_arithmetic | 37 ms | 77 ms | 36 ms | mixed |
| bench_buffer_readwrite | 79 ms | 60 ms | 76 ms | loss vs both |
| bench_array_grow | 8 ms | 28 ms | 19 ms | win vs both |
| bench_string_heavy | 32 ms | 37 ms | 31 ms | mixed |
| bench_numeric_array_numeric | 4 ms | 5 ms | 3 ms | mixed |
| bench_numeric_array_downgrade | 4 ms | 6 ms | 5 ms | win vs both |

## Summary

- Wins versus both peers: **10**
- Losses versus both peers: **3**
- Mixed or tied rows: **11**

> Historical note: the former v0.5.908 single-run commentary is archived in Git history and is not current evidence.
