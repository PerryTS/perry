# Fresh measurements of merged main eee3881c4

Main `eee3881c464bf91ae900e87a42bed072bbdfc95a` (0.5.1529) was freshly built after #10035 incorporated the escaped-record correction. The matched compiler, runtime-static and stdlib-static release build passed. All three benchmark worker objects were relinked against that new archive. These are new merged-main binaries, whose hashes differ from the earlier corrected-R2 build. [Build and GC proof](results/main-eee-validation/README.md); [finite escaped-record comparisons](results/main-eee-finite-validation/README.md).

Apple M1, 8 GiB RAM; Node 26.5.1, Bun 1.3.14. Five repetitions per engine, equal work per row, default GC. CPU is process CPU per call. RSS is whole-process memory, including runtime and inputs. Both measurement windows passed the quiet-host gate. The baseline arm is merged main; perry is the pending opening-scan R3 optimization.

## Parse and stringify suite

Main leads **46/50 CPU rows, 67/86 peak-RSS rows and 31/36 retained-current-RSS rows**. All 200 output checks and 344 measurement groups validate. Repeated-source parse can reuse the source. Large-array full scans are separate from lazy parse and untouched roundtrip.

[Every original parse/stringify and retained-memory row](results/quiet-opening-scan-r3-main-all-r5/comparison.md). **Baseline means merged main** in that table. Its remaining CPU gaps are:

| Fixture | Operation | Main us | Node us | Bun us | Main / best |
|---|---|---:|---:|---:|---:|
| records_array_16k | scan | 63.504 | 40.917 | 35.783 | 1.775x |
| records_array_1m | scan | 3233.464 | 2822.232 | 2225.029 | 1.453x |
| records_array_20m | roundtrip | 98884.000 | 103782.000 | 76070.000 | 1.300x |
| records_array_8m | scan | 24996.250 | 29830.500 | 21424.375 | 1.167x |

## Changing input

Eight equal-size, same-shape sources are preloaded for each engine. Seventeen fixtures change one value; null and the empty object have identical contents in separately loaded strings. Same-source and selection-only controls retain that identical pool. Selection costs are reported without subtraction. This measures changing values within a shape, not an unbounded stream of unique shapes. Array parse may defer materialization; the full-scan rows above consume every record.

All 380 output comparisons and 1140 timing trials validate. All 19 changing-input parse rows for main follow. Peak RSS includes the eight-input pool.

| Fixture | Main CPU us | Node us | Bun us | Main / best | Peak MiB: main / Node / Bun |
|---|---:|---:|---:|---:|---:|
| null | 0.015641 | 0.027623 | 0.019849 | 0.788x | 12.83 / 57.95 / 35.48 |
| string_a | 0.020522 | 0.032002 | 0.023790 | 0.863x | 12.83 / 57.92 / 36.34 |
| empty_object | 0.030402 | 0.045524 | 0.025392 | 1.197x | 32.19 / 59.84 / 69.33 |
| tiny_object | 0.046995 | 0.081353 | 0.046665 | 1.007x | 32.30 / 59.78 / 69.30 |
| small_record | 0.498018 | 0.348725 | 0.254659 | 1.956x | 31.98 / 59.88 / 80.17 |
| object_1k | 0.496410 | 0.539563 | 0.239944 | 2.069x | 32.33 / 62.02 / 71.41 |
| records_array_16k | 18.342598 | 42.235045 | 33.910453 | 0.541x | 63.61 / 66.17 / 70.47 |
| records_array_1m | 1213.424242 | 2757.924242 | 2152.878788 | 0.564x | 76.30 / 128.80 / 93.58 |
| records_object_1m | 2052.300000 | 2694.371429 | 2163.100000 | 0.949x | 66.16 / 96.16 / 85.52 |
| records_array_8m | 10147.714286 | 34091.285714 | 18823.214286 | 0.539x | 173.98 / 335.36 / 227.42 |
| records_object_8m | 15916.000000 | 35677.000000 | 20497.777778 | 0.776x | 303.75 / 290.20 / 228.83 |
| records_array_20m | 43019.625000 | 80232.000000 | 47145.750000 | 0.912x | 729.16 / 683.84 / 473.64 |
| records_object_20m | 42952.750000 | 79428.000000 | 47063.125000 | 0.913x | 729.14 / 683.80 / 473.75 |
| numbers_1m | 1577.493976 | 3145.060241 | 3216.686747 | 0.502x | 66.97 / 111.64 / 77.39 |
| long_string_1m | 108.546769 | 373.039966 | 70.383503 | 1.542x | 90.28 / 173.55 / 154.44 |
| escaped_1m | 1018.572327 | 1726.169811 | 2077.603774 | 0.590x | 65.03 / 89.61 / 77.75 |
| unicode_1m | 148.476023 | 439.574753 | 63.082511 | 2.354x | 62.39 / 160.31 / 147.19 |
| wide_1m | 2706.750000 | 5451.937500 | 4171.109375 | 0.649x | 252.30 / 145.88 / 97.77 |
| heterogeneous_1m | 1426.258427 | 3868.977528 | 2979.393258 | 0.479x | 85.86 / 97.27 / 92.36 |

[Every changing-input, same-source and selection control, including current RSS](results/quiet-opening-scan-r3-main-rotating-r5/comparison.md).

## Pending optimization and remaining work

[Opening-scan R3](OPENING_SCAN.md) uses wider ARM64 opening-byte scans and keeps the empty allocator and long scan tail outside the shared parse path. The original suite retains the same CPU/RSS target counts; full candidate/reference samples and focused replays are linked in that report. The GC policy and construction path are unchanged.

The parity goal remains open. Fresh small objects, large strings, full scans and large-container memory remain relevant targets. [Memory-growth diagnostics](GC_MEMORY_GROWTH.md) show delayed reclamation of old pointer-bearing containers and retained young children. A targeted allocation/parent-liveness experiment is needed before changing collector policy.
