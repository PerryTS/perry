# JSON stringify bounded-writer inlining (R10)

**Rejected for landing.** The longer recheck confirms object parsing +0.78%, large-array stringify +0.80%, and numeric-array stringify +0.70% versus actual main, with separated sample ranges and all 11 paired repetitions slower. Zero-spacing stringify is roughly 9.5× faster than main, but this does not satisfy the no-regression requirement.

Measured source `69455e99520aaee40737948eedad86901d46dd23` on `codex/json-inert-spacer-inline-r10`, based on main `1a9c0de6cb790d2467b0ca22a660870025179b37` (0.5.1531). The exact fresh-main build from R8 is reused by verified hashes; main workers, fixtures and IR were recompiled at R10 paths. This does not measure a newer main revision.

The candidate retains R9’s separate helper for primitive true and numeric ±0 spacers, and changes three existing bounded-writer annotations to `#[inline(always)]`. It retains original fallback arguments and lazy-source admission. GC policy, representation and callback semantics are unchanged. R3/R5/R6/R7/R8 production changes are excluded.

Four engines: main, R10, Node 26.5.1, Bun 1.3.14 on the quiet M1 Mac mini with 8 GiB RAM. The initial 34 controls and original 50-row matrix use seven interleaved fresh-process repetitions. The five-case recheck uses 11 repetitions and four times each corresponding full-matrix iteration count, retaining the same warmup. All 2,572 timed checksums, full-output verification hashes, CPU/RSS vectors and medians, source/worker/input hashes, patches and five terminal quiet windows were independently verified. All samples and outliers are retained.

CPU values below are microseconds per operation. Negative deltas are faster. “Separated” means observed sample ranges do not overlap; it is not a confidence interval or a causal attribution. RSS values are whole-process peak MiB, including input, runtime, output and allocator storage; they are not retained heap. Small RSS differences do not support a general memory claim.

The original 38 parse/stringify rows plus 12 consumption rows give R10 46/50 CPU wins over both peers and 36/50 peak-RSS wins. The remaining CPU gaps are 16 KiB, 1 MiB and 8 MiB scans and the 20 MiB array round trip. The overall performance goal remains open.

The first 34 controls have no separated slowdowns. The full matrix exposes five. Rechecking those five confirms three, clears 16 KiB sparse access (−0.01%, overlapping ranges), and leaves 20 MiB object stringify +0.32% with overlapping ranges but 10/11 pairs slower. The latter is not evidence of no regression. The focus long-string stringify +3.32% and full-matrix −0.73% both have overlapping ranges and different work counts; they are not interchangeable A/B measurements.

## Access

336 trials. Quiet window 2026-09-10T17:27:21Z–2026-09-10T17:27:46Z; one-minute load 1.277→1.629. Quiet gate passed, with no competing workload detected at either boundary; terminal window archived before the next remote operation.

| Workload | Iterations | Main CPU | R10 CPU | Node CPU | Bun CPU | R10 vs main | Ranges | Slower pairs |
|---|---:|---:|---:|---:|---:|---:|---|---:|
| records_array_16k / repeat | 1000000 | 0.018559 | 0.018542 | 0.002756 | 0.004525 | -0.09% | overlap | 1/7 |
| records_array_16k / random | 1000000 | 0.040131 | 0.040364 | 0.006686 | 0.009302 | +0.58% | overlap | 6/7 |
| records_array_16k / fields | 1000000 | 0.084452 | 0.084446 | 0.005334 | 0.009269 | -0.01% | overlap | 3/7 |
| records_array_16k / sequential | 1000000 | 0.035100 | 0.035132 | 0.003918 | 0.005760 | +0.09% | overlap | 5/7 |
| records_array_1m / repeat | 1000000 | 0.018557 | 0.018555 | 0.002793 | 0.004546 | -0.01% | overlap | 5/7 |
| records_array_1m / random | 1000000 | 0.048789 | 0.048828 | 0.010594 | 0.010756 | +0.08% | overlap | 3/7 |
| records_array_1m / fields | 1000000 | 0.107969 | 0.107972 | 0.010819 | 0.014330 | +0.00% | overlap | 3/7 |
| records_array_1m / sequential | 1000000 | 0.040653 | 0.039992 | 0.008164 | 0.007457 | -1.63% | separated gain | 0/7 |
| records_array_20m / repeat | 1000000 | 0.005651 | 0.005656 | 0.003007 | 0.004761 | +0.09% | overlap | 4/7 |
| records_array_20m / random | 1000000 | 0.025906 | 0.025700 | 0.008604 | 0.010089 | -0.80% | overlap | 2/7 |
| records_array_20m / fields | 1000000 | 0.027173 | 0.027167 | 0.011729 | 0.014552 | -0.02% | overlap | 4/7 |
| records_array_20m / sequential | 1000000 | 0.015527 | 0.015563 | 0.006813 | 0.008272 | +0.23% | overlap | 3/7 |

| Workload | Main peak RSS | R10 peak RSS | Node peak RSS | Bun peak RSS |
|---|---:|---:|---:|---:|
| records_array_16k / repeat | 13.031 | 13.156 | 57.938 | 34.797 |
| records_array_16k / random | 13.406 | 13.531 | 57.891 | 35.422 |
| records_array_16k / fields | 13.156 | 13.281 | 58.000 | 35.953 |
| records_array_16k / sequential | 13.156 | 13.281 | 57.875 | 35.234 |
| records_array_1m / repeat | 17.547 | 17.672 | 63.750 | 37.906 |
| records_array_1m / random | 18.469 | 18.594 | 66.625 | 38.891 |
| records_array_1m / fields | 18.266 | 18.375 | 66.781 | 40.297 |
| records_array_1m / sequential | 18.266 | 18.375 | 66.625 | 39.188 |
| records_array_20m / repeat | 96.906 | 97.047 | 194.719 | 93.906 |
| records_array_20m / random | 96.922 | 97.062 | 194.781 | 94.734 |
| records_array_20m / fields | 96.922 | 97.062 | 195.047 | 95.328 |
| records_array_20m / sequential | 96.922 | 97.047 | 194.859 | 94.578 |

[Timed samples](results/quiet-inert-spacer-inline-r10-access/timing.jsonl), [full-output verification](results/quiet-inert-spacer-inline-r10-access/verify.jsonl), [sample vectors and medians](results/quiet-inert-spacer-inline-r10-access/summary.json), [quiet window](results/quiet-inert-spacer-inline-r10-access/window.json).

## Focus

420 trials. Quiet window 2026-09-10T17:27:47Z–2026-09-10T17:31:25Z; one-minute load 1.629→2.076. Quiet gate passed, with no competing workload detected at either boundary; terminal window archived before the next remote operation.

| Workload | Iterations | Main CPU | R10 CPU | Node CPU | Bun CPU | R10 vs main | Ranges | Slower pairs |
|---|---:|---:|---:|---:|---:|---:|---|---:|
| records_array_16k / scan | 4000 | 58.475000 | 58.571500 | 42.001000 | 35.565250 | +0.17% | overlap | 4/7 |
| records_array_1m / scan | 200 | 2901.365000 | 2917.350000 | 2791.270000 | 2204.390000 | +0.55% | overlap | 7/7 |
| records_array_8m / scan | 32 | 23837.156250 | 23893.593750 | 31707.593750 | 21680.562500 | +0.24% | overlap | 6/7 |
| records_array_20m / scan | 16 | 54555.937500 | 54597.250000 | 85314.250000 | 51674.125000 | +0.08% | overlap | 5/7 |
| records_array_20m / roundtrip | 8 | 105395.750000 | 105490.250000 | 98171.000000 | 68500.375000 | +0.09% | overlap | 5/7 |
| records_array_1m / parse | 200 | 911.150000 | 910.100000 | 2763.325000 | 2148.650000 | -0.12% | overlap | 2/7 |
| records_array_1m / stringify | 256 | 639.992188 | 643.160156 | 841.984375 | 963.003906 | +0.50% | overlap | 6/7 |
| small_record / parse | 2000000 | 0.096495 | 0.096488 | 0.354269 | 0.244526 | -0.01% | overlap | 2/7 |
| small_record / stringify | 10000000 | 0.042137 | 0.042450 | 0.107757 | 0.118217 | +0.74% | overlap | 7/7 |
| long_string_1m / stringify | 4096 | 32.286377 | 33.359863 | 99.166504 | 92.816650 | +3.32% | overlap | 4/7 |
| null / stringify | 20000000 | 0.006566 | 0.006254 | 0.026317 | 0.032566 | -4.74% | separated gain | 0/7 |
| string_a / stringify | 20000000 | 0.009693 | 0.009384 | 0.029206 | 0.033411 | -3.18% | separated gain | 0/7 |
| empty_object / stringify | 10000000 | 0.017609 | 0.017464 | 0.031169 | 0.033688 | -0.83% | separated gain | 0/7 |
| tiny_object / stringify | 10000000 | 0.030455 | 0.030400 | 0.036367 | 0.044563 | -0.18% | overlap | 1/7 |
| object_1k / stringify | 2000000 | 0.111721 | 0.111696 | 0.190091 | 0.207293 | -0.02% | overlap | 3/7 |

| Workload | Main peak RSS | R10 peak RSS | Node peak RSS | Bun peak RSS |
|---|---:|---:|---:|---:|
| records_array_16k / scan | 216.125 | 216.141 | 61.953 | 77.422 |
| records_array_1m / scan | 413.703 | 413.734 | 130.359 | 99.609 |
| records_array_8m / scan | 530.234 | 530.250 | 313.734 | 135.625 |
| records_array_20m / scan | 691.422 | 691.438 | 529.547 | 325.641 |
| records_array_20m / roundtrip | 506.328 | 506.359 | 494.094 | 319.156 |
| records_array_1m / parse | 67.094 | 67.125 | 125.672 | 95.031 |
| records_array_1m / stringify | 63.000 | 63.047 | 130.219 | 103.719 |
| small_record / parse | 81.344 | 81.375 | 59.656 | 79.891 |
| small_record / stringify | 33.328 | 33.328 | 59.875 | 394.609 |
| long_string_1m / stringify | 54.469 | 54.516 | 255.484 | 163.703 |
| null / stringify | 13.000 | 13.000 | 59.688 | 134.203 |
| string_a / stringify | 13.000 | 13.000 | 59.688 | 134.188 |
| empty_object / stringify | 13.094 | 13.094 | 59.703 | 134.188 |
| tiny_object / stringify | 32.312 | 32.328 | 59.734 | 134.188 |
| object_1k / stringify | 33.344 | 33.344 | 61.906 | 70.938 |

[Timed samples](results/quiet-inert-spacer-inline-r10-focus/timing.jsonl), [full-output verification](results/quiet-inert-spacer-inline-r10-focus/verify.jsonl), [sample vectors and medians](results/quiet-inert-spacer-inline-r10-focus/summary.json), [quiet window](results/quiet-inert-spacer-inline-r10-focus/window.json).

## Options

196 trials. Quiet window 2026-09-10T17:22:17Z–2026-09-10T17:23:32Z; one-minute load 1.339→2.066. Quiet gate passed, with no competing workload detected at either boundary; terminal window archived before the next remote operation.

| Workload | Iterations | Main CPU | R10 CPU | Node CPU | Bun CPU | R10 vs main | Ranges | Slower pairs |
|---|---:|---:|---:|---:|---:|---:|---|---:|
| small_record / plain | 2000000 | 0.044622 | 0.044630 | 0.108508 | 0.119942 | +0.02% | overlap | 3/7 |
| small_record / dynamic-zero | 2000000 | 0.436983 | 0.046197 | 0.247395 | 0.394184 | -89.43% | separated gain | 0/7 |
| small_record / zero | 2000000 | 0.436304 | 0.046046 | 0.247363 | 0.393449 | -89.45% | separated gain | 0/7 |
| small_record / pretty | 1000000 | 0.482933 | 0.483783 | 0.304272 | 0.524928 | +0.18% | overlap | 4/7 |
| small_record / keys | 1000000 | 0.459774 | 0.452638 | 0.596547 | 0.484087 | -1.55% | separated gain | 0/7 |
| small_record / callback | 200000 | 0.948295 | 0.951465 | 0.605185 | 0.583215 | +0.33% | overlap | 5/7 |
| records_array_16k / pretty | 1000 | 46.147000 | 45.602000 | 33.247000 | 45.789000 | -1.18% | separated gain | 0/7 |

| Workload | Main peak RSS | R10 peak RSS | Node peak RSS | Bun peak RSS |
|---|---:|---:|---:|---:|
| small_record / plain | 33.438 | 33.391 | 59.641 | 378.578 |
| small_record / dynamic-zero | 33.781 | 33.422 | 59.609 | 378.625 |
| small_record / zero | 33.812 | 33.406 | 59.609 | 378.609 |
| small_record / pretty | 33.781 | 33.766 | 59.625 | 239.359 |
| small_record / keys | 33.750 | 33.734 | 59.625 | 207.781 |
| small_record / callback | 32.484 | 32.484 | 59.656 | 97.703 |
| records_array_16k / pretty | 35.078 | 35.078 | 57.000 | 49.734 |

[Timed samples](results/quiet-inert-spacer-inline-r10-options/timing.jsonl), [full-output verification](results/quiet-inert-spacer-inline-r10-options/verify.jsonl), [sample vectors and medians](results/quiet-inert-spacer-inline-r10-options/summary.json), [quiet window](results/quiet-inert-spacer-inline-r10-options/window.json).

## Full

1400 trials. Quiet window 2026-09-10T17:32:28Z–2026-09-10T17:39:17Z; one-minute load 2.080→1.657. Quiet gate passed, with no competing workload detected at either boundary; terminal window archived before the next remote operation.

| Workload | Iterations | Main CPU | R10 CPU | Node CPU | Bun CPU | R10 vs main | Ranges | Slower pairs |
|---|---:|---:|---:|---:|---:|---:|---|---:|
| null / parse | 2000000 | 0.010632 | 0.010631 | 0.027348 | 0.019152 | -0.00% | overlap | 5/7 |
| null / stringify | 2000000 | 0.006567 | 0.006256 | 0.027209 | 0.031236 | -4.74% | separated gain | 0/7 |
| string_a / parse | 2000000 | 0.012826 | 0.012823 | 0.031739 | 0.022604 | -0.02% | overlap | 2/7 |
| string_a / stringify | 2000000 | 0.009691 | 0.009380 | 0.030079 | 0.032065 | -3.21% | separated gain | 0/7 |
| empty_object / parse | 2000000 | 0.023361 | 0.023401 | 0.045164 | 0.024125 | +0.17% | overlap | 4/7 |
| empty_object / stringify | 2000000 | 0.017601 | 0.017447 | 0.031974 | 0.032573 | -0.87% | separated gain | 0/7 |
| tiny_object / parse | 2000000 | 0.036802 | 0.036755 | 0.081713 | 0.045257 | -0.13% | overlap | 4/7 |
| tiny_object / stringify | 2000000 | 0.033981 | 0.033843 | 0.037143 | 0.043131 | -0.41% | overlap | 1/7 |
| small_record / parse | 1581369 | 0.098486 | 0.098523 | 0.326712 | 0.244245 | +0.04% | overlap | 3/7 |
| small_record / stringify | 2000000 | 0.045713 | 0.046031 | 0.108040 | 0.119653 | +0.69% | overlap | 7/7 |
| object_1k / parse | 1961848 | 0.082034 | 0.081969 | 0.535029 | 0.227639 | -0.08% | overlap | 2/7 |
| object_1k / stringify | 1036867 | 0.115200 | 0.115165 | 0.193466 | 0.210903 | -0.03% | overlap | 5/7 |
| records_array_16k / parse | 11284 | 13.579316 | 13.594647 | 41.032258 | 33.837203 | +0.11% | overlap | 3/7 |
| records_array_16k / stringify | 12949 | 10.982933 | 11.047108 | 13.144490 | 23.230829 | +0.58% | overlap | 6/7 |
| records_array_16k / sparse | 7868 | 19.971657 | 20.058338 | 39.328038 | 33.957041 | +0.43% | separated slowdown | 7/7 |
| records_array_16k / scan | 3761 | 58.759107 | 58.774528 | 39.980590 | 35.726137 | +0.03% | overlap | 4/7 |
| records_array_16k / roundtrip | 7659 | 20.998172 | 20.939287 | 53.207729 | 57.681029 | -0.28% | overlap | 0/7 |
| records_array_1m / parse | 170 | 918.758824 | 917.294118 | 2748.864706 | 2139.000000 | -0.16% | overlap | 2/7 |
| records_array_1m / stringify | 229 | 645.087336 | 649.838428 | 848.423581 | 966.441048 | +0.74% | overlap | 6/7 |
| records_array_1m / sparse | 161 | 968.273292 | 968.496894 | 2694.689441 | 2178.409938 | +0.02% | overlap | 4/7 |
| records_array_1m / scan | 69 | 2936.405797 | 2948.101449 | 2784.420290 | 2225.420290 | +0.40% | overlap | 4/7 |
| records_array_1m / roundtrip | 115 | 1411.860870 | 1411.060870 | 3587.269565 | 3110.260870 | -0.06% | overlap | 5/7 |
| records_object_1m / parse | 81 | 2008.172840 | 2022.962963 | 2852.654321 | 2152.000000 | +0.74% | separated slowdown | 7/7 |
| records_object_1m / stringify | 229 | 648.864629 | 651.065502 | 847.056769 | 966.109170 | +0.34% | overlap | 3/7 |
| records_array_8m / parse | 16 | 8273.562500 | 8303.062500 | 32968.687500 | 20893.437500 | +0.36% | overlap | 6/7 |
| records_array_8m / stringify | 29 | 4725.068966 | 4768.931034 | 7236.655172 | 8194.551724 | +0.93% | separated slowdown | 7/7 |
| records_array_8m / sparse | 15 | 8849.266667 | 8839.333333 | 32689.066667 | 21555.400000 | -0.11% | overlap | 4/7 |
| records_array_8m / scan | 8 | 22707.000000 | 22725.375000 | 30845.250000 | 21444.250000 | +0.08% | overlap | 4/7 |
| records_array_8m / roundtrip | 11 | 13110.545455 | 13116.454545 | 36670.363636 | 27495.454545 | +0.05% | overlap | 5/7 |
| records_object_8m / parse | 10 | 15950.600000 | 15974.400000 | 34502.000000 | 20948.300000 | +0.15% | overlap | 4/7 |
| records_object_8m / stringify | 29 | 4770.758621 | 4779.965517 | 7272.793103 | 8189.068966 | +0.19% | overlap | 4/7 |
| records_array_20m / parse | 4 | 39387.500000 | 39555.250000 | 94626.500000 | 53127.000000 | +0.43% | overlap | 6/7 |
| records_array_20m / stringify | 12 | 11749.416667 | 11797.166667 | 17280.583333 | 20111.000000 | +0.41% | overlap | 6/7 |
| records_array_20m / sparse | 4 | 39393.750000 | 39514.500000 | 93925.750000 | 53160.750000 | +0.31% | overlap | 6/7 |
| records_array_20m / scan | 4 | 40389.250000 | 40420.000000 | 90973.250000 | 53846.250000 | +0.08% | overlap | 4/7 |
| records_array_20m / roundtrip | 2 | 98613.000000 | 98716.500000 | 101181.000000 | 76638.500000 | +0.10% | overlap | 5/7 |
| records_object_20m / parse | 4 | 39433.500000 | 39522.750000 | 93249.750000 | 53129.000000 | +0.23% | overlap | 5/7 |
| records_object_20m / stringify | 12 | 11712.250000 | 11744.666667 | 17250.666667 | 20095.083333 | +0.28% | separated slowdown | 7/7 |
| numbers_1m / parse | 102 | 1581.362745 | 1550.529412 | 3128.529412 | 3210.107843 | -1.95% | separated gain | 0/7 |
| numbers_1m / stringify | 147 | 1013.115646 | 1019.190476 | 1974.183673 | 2943.238095 | +0.60% | separated slowdown | 7/7 |
| long_string_1m / parse | 2806 | 0.349608 | 0.349608 | 364.279401 | 64.471846 | +0.00% | overlap | 2/7 |
| long_string_1m / stringify | 1922 | 35.625390 | 35.364724 | 102.207596 | 94.889178 | -0.73% | overlap | 3/7 |
| escaped_1m / parse | 158 | 981.227848 | 981.310127 | 1763.075949 | 2072.594937 | +0.01% | overlap | 4/7 |
| escaped_1m / stringify | 166 | 884.957831 | 885.963855 | 1898.150602 | 2089.385542 | +0.11% | overlap | 5/7 |
| unicode_1m / parse | 2786 | 0.349605 | 0.348887 | 440.892678 | 57.877961 | -0.21% | overlap | 3/7 |
| unicode_1m / stringify | 2475 | 28.500606 | 28.608889 | 409.594343 | 448.098586 | +0.38% | overlap | 6/7 |
| wide_1m / parse | 64 | 2886.671875 | 2887.218750 | 5262.656250 | 4135.921875 | +0.02% | overlap | 3/7 |
| wide_1m / stringify | 232 | 633.297414 | 634.012931 | 6364.762931 | 663.935345 | +0.11% | overlap | 5/7 |
| heterogeneous_1m / parse | 142 | 1118.063380 | 1118.387324 | 3921.021127 | 2942.767606 | +0.03% | overlap | 5/7 |
| heterogeneous_1m / stringify | 218 | 717.940367 | 715.009174 | 896.426606 | 1055.325688 | -0.41% | separated gain | 0/7 |

| Workload | Main peak RSS | R10 peak RSS | Node peak RSS | Bun peak RSS |
|---|---:|---:|---:|---:|
| null / parse | 12.797 | 12.797 | 57.719 | 35.750 |
| null / stringify | 13.000 | 13.000 | 59.625 | 129.797 |
| string_a / parse | 12.797 | 12.797 | 57.703 | 36.125 |
| string_a / stringify | 13.000 | 13.000 | 59.656 | 129.812 |
| empty_object / parse | 32.125 | 32.156 | 59.516 | 69.062 |
| empty_object / stringify | 13.094 | 13.094 | 59.656 | 129.812 |
| tiny_object / parse | 32.266 | 32.281 | 59.625 | 69.109 |
| tiny_object / stringify | 32.312 | 32.328 | 59.656 | 129.812 |
| small_record / parse | 79.562 | 79.547 | 59.641 | 79.891 |
| small_record / stringify | 33.312 | 33.328 | 59.781 | 378.703 |
| object_1k / parse | 64.938 | 64.922 | 61.828 | 71.281 |
| object_1k / stringify | 33.344 | 33.344 | 61.781 | 70.859 |
| records_array_16k / parse | 63.422 | 63.453 | 65.906 | 70.625 |
| records_array_16k / stringify | 33.516 | 33.547 | 61.906 | 71.047 |
| records_array_16k / sparse | 72.203 | 72.234 | 66.062 | 70.656 |
| records_array_16k / scan | 204.422 | 204.438 | 61.984 | 77.406 |
| records_array_16k / roundtrip | 69.219 | 69.266 | 65.906 | 70.906 |
| records_array_1m / parse | 67.094 | 67.125 | 125.625 | 88.750 |
| records_array_1m / stringify | 63.000 | 63.047 | 129.453 | 103.688 |
| records_array_1m / sparse | 67.609 | 67.641 | 125.594 | 97.297 |
| records_array_1m / scan | 158.484 | 158.500 | 97.641 | 83.203 |
| records_array_1m / roundtrip | 61.469 | 61.516 | 152.125 | 93.328 |
| records_object_1m / parse | 66.422 | 66.438 | 92.984 | 79.172 |
| records_object_1m / stringify | 63.062 | 63.078 | 129.453 | 103.703 |
| records_array_8m / parse | 108.812 | 108.844 | 265.938 | 141.266 |
| records_array_8m / stringify | 121.125 | 121.172 | 212.562 | 176.844 |
| records_array_8m / sparse | 109.141 | 109.172 | 265.922 | 187.969 |
| records_array_8m / scan | 186.703 | 186.719 | 230.078 | 133.578 |
| records_array_8m / roundtrip | 129.250 | 129.297 | 267.438 | 149.375 |
| records_object_8m / parse | 187.312 | 187.328 | 244.375 | 131.547 |
| records_object_8m / stringify | 121.141 | 121.188 | 212.625 | 196.625 |
| records_array_20m / parse | 255.422 | 255.438 | 359.844 | 220.828 |
| records_array_20m / stringify | 235.188 | 235.250 | 459.547 | 304.938 |
| records_array_20m / sparse | 255.422 | 255.438 | 359.734 | 220.859 |
| records_array_20m / scan | 255.438 | 255.453 | 330.703 | 225.797 |
| records_array_20m / roundtrip | 295.375 | 295.375 | 342.016 | 255.719 |
| records_object_20m / parse | 255.422 | 255.422 | 359.781 | 220.719 |
| records_object_20m / stringify | 235.234 | 235.266 | 459.516 | 304.797 |
| numbers_1m / parse | 62.766 | 62.797 | 104.156 | 68.234 |
| numbers_1m / stringify | 57.828 | 57.891 | 113.078 | 74.281 |
| long_string_1m / parse | 16.422 | 16.453 | 209.219 | 158.469 |
| long_string_1m / stringify | 54.500 | 54.484 | 183.625 | 155.641 |
| escaped_1m / parse | 58.391 | 58.422 | 102.609 | 67.891 |
| escaped_1m / stringify | 54.875 | 54.922 | 124.828 | 74.812 |
| unicode_1m / parse | 15.766 | 15.781 | 203.734 | 156.203 |
| unicode_1m / stringify | 53.844 | 53.891 | 186.328 | 153.562 |
| wide_1m / parse | 227.688 | 227.703 | 121.156 | 95.094 |
| wide_1m / stringify | 68.656 | 68.672 | 117.891 | 84.688 |
| heterogeneous_1m / parse | 62.547 | 62.578 | 92.797 | 99.594 |
| heterogeneous_1m / stringify | 61.703 | 61.750 | 129.000 | 104.953 |

[Timed samples](results/quiet-inert-spacer-inline-r10-full/timing.jsonl), [full-output verification](results/quiet-inert-spacer-inline-r10-full/verify.jsonl), [sample vectors and medians](results/quiet-inert-spacer-inline-r10-full/summary.json), [quiet window](results/quiet-inert-spacer-inline-r10-full/window.json).

## Full longer recheck

220 trials. Quiet window 2026-09-10T17:42:13Z–2026-09-10T17:45:27Z; one-minute load 1.465→2.393. Quiet gate passed, with no competing workload detected at either boundary; terminal window archived before the next remote operation.

| Workload | Iterations | Main CPU | R10 CPU | Node CPU | Bun CPU | R10 vs main | Ranges | Slower pairs |
|---|---:|---:|---:|---:|---:|---:|---|---:|
| records_array_16k / sparse | 31472 | 20.264648 | 20.262138 | 38.822032 | 33.770653 | -0.01% | overlap | 6/11 |
| records_object_1m / parse | 324 | 1867.904321 | 1882.385802 | 2666.641975 | 2139.694444 | +0.78% | separated slowdown | 11/11 |
| records_array_8m / stringify | 116 | 4662.094828 | 4699.353448 | 6811.758621 | 7998.060345 | +0.80% | separated slowdown | 11/11 |
| records_object_20m / stringify | 48 | 11571.062500 | 11607.687500 | 17978.395833 | 20020.062500 | +0.32% | overlap | 10/11 |
| numbers_1m / stringify | 588 | 992.345238 | 999.287415 | 1946.187075 | 2938.440476 | +0.70% | separated slowdown | 11/11 |

| Workload | Main peak RSS | R10 peak RSS | Node peak RSS | Bun peak RSS |
|---|---:|---:|---:|---:|
| records_array_16k / sparse | 76.312 | 76.344 | 74.250 | 74.109 |
| records_object_1m / parse | 71.172 | 71.188 | 125.609 | 107.797 |
| records_array_8m / stringify | 121.125 | 121.188 | 286.672 | 237.469 |
| records_object_20m / stringify | 235.219 | 235.266 | 696.297 | 457.344 |
| numbers_1m / stringify | 57.828 | 57.891 | 150.453 | 101.516 |

[Timed samples](results/quiet-inert-spacer-inline-r10-recheck-full/timing.jsonl), [full-output verification](results/quiet-inert-spacer-inline-r10-recheck-full/verify.jsonl), [sample vectors and medians](results/quiet-inert-spacer-inline-r10-recheck-full/summary.json), [quiet window](results/quiet-inert-spacer-inline-r10-recheck-full/window.json).

## Machine code and validation

- The three writers are again inlined in the public full entry. Its frame grows from main’s 272 to 336 bytes, and the new helper has a 128-byte frame. Both public entries are 64-byte aligned. These observations do not establish the cause of the broad slowdowns.
- Exact production build: `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static`, from frozen clean source, 326.68 seconds. All three artifact mtimes are after build start; compiler/runtime/stdlib hashes are recorded. All four main/candidate generated worker objects match byte-for-byte and link the corresponding frozen runtime. Sixty remote hashes, including all 19 fixture inputs, were verified.
- `RUST_TEST_THREADS=1 cargo test --release -p perry-runtime --lib json`: 292 tests pass on final source.
- Main and candidate each pass 19 Node fixture checks across auto/tape/direct parsing and normal/scheduled/full GC, including callback-only pressure; each passes 14 options checks. All scheduled runs assert positive protected page sets and movement. The new spacer fixture has 1,190 protected sets and 44,380/44,455/44,380 moved objects; callback-only has 16 sets and 13,305 moved objects.
- Full native static checking retains nine UNSUPPRESSED findings: eight unrooted global values and one stale allocation value. All match actual main fingerprints. Twelve IR files match after removing only the first native ModuleID path comment; shadow IR needs no normalization. Both shadow variants pass. Native ordinary and callback-only controls have zero findings. No allowlist or stale-value allowance was increased; this is not an all-clean static result.
- Script lint: 73/74 pass; public benchmark evidence freshness fails. Compile tier and two CI-only checks are skipped. The Rust file cap passes. Full CI is not claimed.

[Build provenance](results/inert-spacer-inline-r10-validation/build-provenance.json), [reference main](results/inert-spacer-inline-r10-validation/reference-main.json), [worker objects](results/inert-spacer-inline-r10-validation/worker-object-comparison.json), [machine observations](results/inert-spacer-inline-r10-validation/machine-observations.json), [behavioral checks](results/inert-spacer-inline-r10-validation/candidate-fixture-validation.json), [options checks](results/inert-spacer-inline-r10-validation/candidate-options-validation.json), [root comparison](results/inert-spacer-inline-r10-validation/root-comparison.json), [lint log](results/inert-spacer-inline-r10-validation/script-lint.log.gz).

## Known limitations and remaining work

All 24 isolated lazy-array probes preserve actual main outcomes and complete stdout. All twelve two-record cases match Node. At 180 records, plain canonical input passes, plain whitespace and duplicate-key inputs return noncanonical raw JSON, six zero/true spacing cases crash with SIGSEGV, and three pretty cases pass. Preserving baseline failures is not conformance. Normalizing zero to undefined would widen the faulty raw-source admission, so fallback retains the original spacer.

Fractional spacing preserves the recorded main/Bun versus Node difference for positive values below one. It is an explicit baseline gap, excluded from passing Node checks.

[Lazy baseline](results/inert-spacer-inline-r10-validation/lazy-main-probes.json), [candidate lazy probes](results/inert-spacer-inline-r10-validation/lazy-candidate-probes.json), [fraction baseline](results/inert-spacer-inline-r10-validation/fraction-baseline.json), [candidate fraction probe](results/inert-spacer-inline-r10-validation/candidate-fraction.json).

[Verifier](results/inert-spacer-inline-r10-validation/analyze.py) was run with `--with-full` and `--full-recheck`. [Validation manifest](results/inert-spacer-inline-r10-validation/manifest.json) records every archived artifact and its original and stored hashes. Retained-memory, rotating-input and short-call drivers were prepared but not executed on this rejected candidate. They remain required for a future candidate; their presence in the archive is not a timing or validation claim.

[R9 report](https://github.com/PerryTS/perry/blob/04fc70bbc1194d32ec8bc4d4e1caca5340797567/benchmarks/json_performance/INERT_SPACER_TAIL_R9.md) records the preceding rejected experiment. No PR is opened for R10 and no version bump is made.
