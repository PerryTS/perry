# Isolated borrowed JSON templates (R14)

**Rejected for landing.** Isolating template borrowing on main preserves the small-object gains, but three large-workload slowdowns remain separated in the longer recheck: 1 MiB object parsing +0.97%, 20 MiB array parsing +0.49%, and 20 MiB object parsing +0.57%. All eleven pairs are slower in each. Two identical-main controls are effectively flat and do not explain these differences.

Measured source `de85016c81ce39316408ef755c57eded37dd3202` on `codex/json-template-only-r14`, directly based on actual main `1a9c0de6cb790d2467b0ca22a660870025179b37` (0.5.1531). Only template borrowing, its predicate/construction split, two tests and an experiment fragment are added. R11/R12’s source-length and construction-context changes are excluded; the parser and string constructor match main’s source exactly. Main’s original fresh build was reused by verified hashes, with workers, fixtures and IR recompiled at R14 paths. Remote main was reconfirmed before timing.

The cached plan is borrowed during the existing GC suppression scope. Every mutable object and nested array is still allocated afresh. The collection hook runs before the borrow; the borrow ends before suppression restoration, cleanup and scheduling. Cache bounds, admission, root registration and GC policy remain unchanged. The match predicate is inlined into parse_slow; a separate helper constructs cache hits.

## Results and method

Main, R14, Node 26.5.1 and Bun 1.3.14 ran on the quiet M1 Mac mini with 8 GiB RAM. Every sample and outlier is retained. CPU is microseconds per operation; negative deltas are faster. Separated ranges are an observed sample property, not a confidence interval or proof of cause. Peak RSS is whole-process MiB, including inputs, outputs, runtime and allocator storage; it is not isolated heap size.

The 15 rotating/repeated/selection controls preload eight equal-size sources outside timing and use seven interleaved fresh-process repetitions. Rotating input defeats the existing single-source cache; it is not a fresh source allocation on every call, and selection cost is not subtracted. The full 50 cases cover the original 38 parse/stringify rows plus 12 consumption cases, with seven repetitions and fixed work. The seven-case recheck uses eleven repetitions and four times the initial work, retaining each original warmup.

Valid candidate evidence totals 2,128 timed trials, 60 calibration trials and 385 complete-output checks across three quiet windows. The two diagnostic A/A controls add 176 timings and 20 complete-output checks; neither measures candidate code. All results were archived before the next remote operation.

The first rotating window failed the pre-existing quiet gate: ending one-minute load was 2.594, despite no competing workload being detected at either boundary. All 420 timings, 60 calibrations and 100 verification records are preserved separately and excluded from performance qualification. Its medians were not inspected to select the retry. The same comparison was repeated in a new directory and passed the quiet gate.

[Invalid window](results/quiet-template-only-r14-rotating/window.json), [raw invalid timings](results/quiet-template-only-r14-rotating/timing.jsonl), [exclusion note](results/quiet-template-only-r14-rotating/INVALID_WINDOW.md).

The valid initial controls show repeated-input small-record parsing 9.75% faster; changing-input parsing is 0.13% faster with overlapping ranges. None of the other controls has a separated slowdown. The full run shows small-record parsing 11.30% faster, the 1 KiB object case 12.35% faster and tiny-object parsing 2.06% faster. Perry beats both peers on 46/50 CPU medians and 36/50 peak-RSS medians; those counts do not establish the complete objective.

Six full-run cases have separated slowdowns; one additional case is slower in every pair with overlapping ranges. The longer recheck retains three separated slowdowns. Its other four medians remain positive with overlapping ranges, with ten or eleven slower pairs. This candidate does not establish the required absence of regressions.

## Changing-input and repeated-input controls

Window 2026-09-10T19:52:35Z–2026-09-10T19:54:17Z; one-minute load 1.515→2.057. Quiet gate passed; no competing workload detected at either boundary.

| Fixture / operation | Iterations | Main CPU | R14 CPU | Node CPU | Bun CPU | Delta | Ranges | Slower pairs |
|---|---:|---:|---:|---:|---:|---:|---|---:|
| small_record / rotating | 572519 | 0.494097 | 0.493444 | 0.349119 | 0.254839 | -0.132% | overlap | 1/7 |
| small_record / same | 1648351 | 0.098892 | 0.089246 | 0.334112 | 0.245104 | -9.754% | separated gain | 0/7 |
| small_record / select | 2000000 | 0.009433 | 0.009427 | 0.002661 | 0.003636 | -0.074% | overlap | 2/7 |
| records_array_1m / rotating | 174 | 942.425287 | 943.419540 | 2641.310345 | 2156.097701 | +0.105% | overlap | 5/7 |
| records_array_1m / same | 175 | 938.754286 | 938.051429 | 2684.691429 | 2152.862857 | -0.075% | overlap | 3/7 |
| records_array_1m / select | 2000000 | 0.011699 | 0.011664 | 0.003112 | 0.003884 | -0.295% | overlap | 4/7 |
| long_string_1m / rotating | 1256 | 100.089172 | 99.947452 | 373.199841 | 69.579618 | -0.142% | overlap | 2/7 |
| long_string_1m / same | 3118 | 0.361450 | 0.359846 | 369.029185 | 66.080821 | -0.444% | overlap | 3/7 |
| long_string_1m / select | 2000000 | 0.012075 | 0.011404 | 0.003099 | 0.003879 | -5.557% | overlap | 1/7 |
| escaped_1m / rotating | 159 | 1011.446541 | 1012.377358 | 1725.396226 | 2077.905660 | +0.092% | overlap | 5/7 |
| escaped_1m / same | 160 | 992.225000 | 993.237500 | 1723.537500 | 2074.493750 | +0.102% | overlap | 5/7 |
| escaped_1m / select | 2000000 | 0.012125 | 0.011428 | 0.003112 | 0.003864 | -5.744% | overlap | 1/7 |
| unicode_1m / rotating | 1396 | 141.865330 | 141.699857 | 439.560172 | 63.012894 | -0.117% | overlap | 1/7 |
| unicode_1m / same | 2822 | 0.356839 | 0.356839 | 438.257619 | 59.110206 | +0.000% | overlap | 3/7 |
| unicode_1m / select | 2000000 | 0.011714 | 0.011433 | 0.003141 | 0.003870 | -2.399% | overlap | 3/7 |

| Fixture / operation | Main peak RSS | R14 peak RSS | Node peak RSS | Bun peak RSS |
|---|---:|---:|---:|---:|
| small_record / rotating | 31.984 | 31.969 | 59.859 | 80.203 |
| small_record / same | 81.312 | 81.297 | 59.641 | 79.906 |
| small_record / select | 12.812 | 12.797 | 57.859 | 35.281 |
| records_array_1m / rotating | 76.297 | 76.281 | 128.844 | 99.875 |
| records_array_1m / same | 76.312 | 76.281 | 128.828 | 100.219 |
| records_array_1m / select | 22.594 | 22.578 | 67.562 | 42.516 |
| long_string_1m / rotating | 90.234 | 90.203 | 176.609 | 123.469 |
| long_string_1m / same | 24.609 | 24.594 | 200.953 | 156.547 |
| long_string_1m / select | 24.328 | 24.312 | 68.969 | 43.766 |
| escaped_1m / rotating | 65.000 | 64.984 | 89.672 | 77.781 |
| escaped_1m / same | 65.000 | 64.984 | 89.625 | 77.750 |
| escaped_1m / select | 23.531 | 23.500 | 68.406 | 43.266 |
| unicode_1m / rotating | 61.531 | 61.516 | 159.562 | 147.234 |
| unicode_1m / same | 22.703 | 22.672 | 212.141 | 147.281 |
| unicode_1m / select | 22.422 | 22.422 | 68.578 | 44.641 |

[Timings](results/quiet-template-only-r14-retry1-rotating/timing.jsonl), [full-output checks](results/quiet-template-only-r14-retry1-rotating/verify.jsonl), [host and worker hashes](results/quiet-template-only-r14-retry1-rotating/host.json), [window](results/quiet-template-only-r14-retry1-rotating/window.json).

## All 50 parse/stringify/consumption cases

Window 2026-09-10T19:55:38Z–2026-09-10T20:02:29Z; one-minute load 1.639→2.142. Quiet gate passed; no competing workload detected at either boundary.

| Fixture / operation | Iterations | Main CPU | R14 CPU | Node CPU | Bun CPU | Delta | Ranges | Slower pairs |
|---|---:|---:|---:|---:|---:|---:|---|---:|
| null / parse | 2000000 | 0.010629 | 0.010627 | 0.027362 | 0.019092 | -0.024% | overlap | 3/7 |
| null / stringify | 2000000 | 0.006567 | 0.006566 | 0.027203 | 0.031218 | -0.015% | overlap | 3/7 |
| string_a / parse | 2000000 | 0.012823 | 0.012828 | 0.031727 | 0.022614 | +0.047% | overlap | 3/7 |
| string_a / stringify | 2000000 | 0.009683 | 0.009690 | 0.030075 | 0.032031 | +0.067% | overlap | 4/7 |
| empty_object / parse | 2000000 | 0.023422 | 0.023436 | 0.045173 | 0.024179 | +0.058% | overlap | 5/7 |
| empty_object / stringify | 2000000 | 0.017632 | 0.017610 | 0.031985 | 0.032530 | -0.125% | overlap | 1/7 |
| tiny_object / parse | 2000000 | 0.036844 | 0.036085 | 0.080505 | 0.045196 | -2.059% | separated gain | 0/7 |
| tiny_object / stringify | 2000000 | 0.033937 | 0.033947 | 0.037137 | 0.043154 | +0.032% | overlap | 3/7 |
| small_record / parse | 1581369 | 0.098550 | 0.087410 | 0.333545 | 0.245613 | -11.304% | separated gain | 0/7 |
| small_record / stringify | 2000000 | 0.045767 | 0.045676 | 0.108103 | 0.119142 | -0.199% | overlap | 1/7 |
| object_1k / parse | 1961848 | 0.081937 | 0.071816 | 0.531713 | 0.227626 | -12.351% | separated gain | 0/7 |
| object_1k / stringify | 1036867 | 0.115196 | 0.115295 | 0.193855 | 0.211060 | +0.086% | overlap | 4/7 |
| records_array_16k / parse | 11284 | 13.589596 | 13.590305 | 39.392946 | 33.832949 | +0.005% | overlap | 3/7 |
| records_array_16k / stringify | 12949 | 10.975828 | 10.982779 | 13.142096 | 23.231292 | +0.063% | overlap | 5/7 |
| records_array_16k / sparse | 7868 | 19.978521 | 19.960981 | 39.209329 | 33.966065 | -0.088% | overlap | 2/7 |
| records_array_16k / scan | 3761 | 58.834884 | 58.827174 | 40.640787 | 35.735177 | -0.013% | overlap | 4/7 |
| records_array_16k / roundtrip | 7659 | 20.976759 | 20.929495 | 53.333333 | 57.693694 | -0.225% | overlap | 0/7 |
| records_array_1m / parse | 170 | 918.776471 | 918.182353 | 2634.711765 | 2141.782353 | -0.065% | overlap | 3/7 |
| records_array_1m / stringify | 229 | 646.646288 | 647.100437 | 849.550218 | 965.956332 | +0.070% | overlap | 4/7 |
| records_array_1m / sparse | 161 | 968.037267 | 968.062112 | 2849.285714 | 2180.142857 | +0.003% | overlap | 1/7 |
| records_array_1m / scan | 69 | 2926.565217 | 2943.884058 | 2801.869565 | 2218.463768 | +0.592% | overlap | 7/7 |
| records_array_1m / roundtrip | 115 | 1413.113043 | 1415.982609 | 3516.408696 | 3105.800000 | +0.203% | overlap | 5/7 |
| records_object_1m / parse | 81 | 2009.728395 | 2029.506173 | 2851.444444 | 2152.333333 | +0.984% | separated slowdown | 7/7 |
| records_object_1m / stringify | 229 | 648.895197 | 648.362445 | 847.344978 | 966.432314 | -0.082% | overlap | 4/7 |
| records_array_8m / parse | 16 | 8304.437500 | 8286.125000 | 32818.187500 | 20766.125000 | -0.221% | overlap | 2/7 |
| records_array_8m / stringify | 29 | 4732.000000 | 4730.137931 | 7247.137931 | 8199.862069 | -0.039% | overlap | 3/7 |
| records_array_8m / sparse | 15 | 8865.800000 | 8861.800000 | 32651.866667 | 21446.066667 | -0.045% | overlap | 2/7 |
| records_array_8m / scan | 8 | 22689.625000 | 22752.375000 | 30559.625000 | 21471.000000 | +0.277% | overlap | 6/7 |
| records_array_8m / roundtrip | 11 | 13119.363636 | 13144.272727 | 37239.545455 | 27357.727273 | +0.190% | overlap | 6/7 |
| records_object_8m / parse | 10 | 15926.800000 | 16011.100000 | 34297.700000 | 20889.600000 | +0.529% | separated slowdown | 7/7 |
| records_object_8m / stringify | 29 | 4761.482759 | 4768.689655 | 7147.103448 | 8160.689655 | +0.151% | overlap | 4/7 |
| records_array_20m / parse | 4 | 39455.000000 | 39659.750000 | 92375.250000 | 53043.000000 | +0.519% | separated slowdown | 7/7 |
| records_array_20m / stringify | 12 | 11760.500000 | 11743.083333 | 17269.666667 | 20036.833333 | -0.148% | overlap | 1/7 |
| records_array_20m / sparse | 4 | 39395.750000 | 39681.000000 | 94369.750000 | 53105.750000 | +0.724% | separated slowdown | 7/7 |
| records_array_20m / scan | 4 | 40408.750000 | 40691.750000 | 90382.500000 | 54128.250000 | +0.700% | separated slowdown | 7/7 |
| records_array_20m / roundtrip | 2 | 99073.500000 | 99380.000000 | 103757.000000 | 76601.000000 | +0.309% | overlap | 6/7 |
| records_object_20m / parse | 4 | 39469.250000 | 39742.250000 | 97859.250000 | 53106.250000 | +0.692% | separated slowdown | 7/7 |
| records_object_20m / stringify | 12 | 11729.083333 | 11714.250000 | 17284.583333 | 20179.500000 | -0.126% | overlap | 2/7 |
| numbers_1m / parse | 102 | 1586.794118 | 1582.147059 | 3126.205882 | 3193.980392 | -0.293% | overlap | 3/7 |
| numbers_1m / stringify | 147 | 1014.727891 | 1014.054422 | 1976.047619 | 2943.414966 | -0.066% | overlap | 4/7 |
| long_string_1m / parse | 2806 | 0.349608 | 0.347470 | 364.564505 | 64.547042 | -0.612% | overlap | 0/7 |
| long_string_1m / stringify | 1922 | 35.252862 | 34.905307 | 102.127471 | 94.626431 | -0.986% | overlap | 1/7 |
| escaped_1m / parse | 158 | 980.620253 | 980.797468 | 1763.354430 | 2073.727848 | +0.018% | overlap | 5/7 |
| escaped_1m / stringify | 166 | 884.710843 | 884.873494 | 1899.927711 | 2086.837349 | +0.018% | overlap | 3/7 |
| unicode_1m / parse | 2786 | 0.349246 | 0.347452 | 441.040201 | 58.006461 | -0.514% | overlap | 0/7 |
| unicode_1m / stringify | 2475 | 27.008485 | 27.815758 | 409.961212 | 448.004848 | +2.989% | overlap | 4/7 |
| wide_1m / parse | 64 | 2894.640625 | 2887.203125 | 5268.140625 | 4144.281250 | -0.257% | overlap | 2/7 |
| wide_1m / stringify | 232 | 634.198276 | 634.125000 | 6335.689655 | 663.375000 | -0.012% | overlap | 4/7 |
| heterogeneous_1m / parse | 142 | 1117.408451 | 1117.957746 | 3856.852113 | 2939.147887 | +0.049% | overlap | 4/7 |
| heterogeneous_1m / stringify | 218 | 718.261468 | 718.541284 | 897.830275 | 1055.816514 | +0.039% | overlap | 2/7 |

| Fixture / operation | Main peak RSS | R14 peak RSS | Node peak RSS | Bun peak RSS |
|---|---:|---:|---:|---:|
| null / parse | 12.812 | 12.797 | 57.688 | 35.766 |
| null / stringify | 13.031 | 13.000 | 59.656 | 129.828 |
| string_a / parse | 12.812 | 12.797 | 57.750 | 36.125 |
| string_a / stringify | 13.016 | 13.000 | 59.672 | 129.812 |
| empty_object / parse | 32.141 | 32.141 | 59.547 | 69.078 |
| empty_object / stringify | 13.109 | 13.094 | 59.625 | 129.797 |
| tiny_object / parse | 32.266 | 32.297 | 59.625 | 69.094 |
| tiny_object / stringify | 32.328 | 32.328 | 59.688 | 129.812 |
| small_record / parse | 79.578 | 79.578 | 59.641 | 79.906 |
| small_record / stringify | 33.344 | 33.344 | 59.734 | 378.688 |
| object_1k / parse | 64.953 | 64.922 | 61.859 | 71.281 |
| object_1k / stringify | 33.359 | 33.359 | 61.766 | 70.875 |
| records_array_16k / parse | 63.453 | 63.438 | 65.859 | 70.625 |
| records_array_16k / stringify | 33.531 | 33.531 | 61.891 | 71.047 |
| records_array_16k / sparse | 72.219 | 72.219 | 66.031 | 70.656 |
| records_array_16k / scan | 204.453 | 204.453 | 61.938 | 77.406 |
| records_array_16k / roundtrip | 68.781 | 69.234 | 65.938 | 70.906 |
| records_array_1m / parse | 67.109 | 67.109 | 125.641 | 88.766 |
| records_array_1m / stringify | 63.016 | 63.016 | 129.375 | 103.672 |
| records_array_1m / sparse | 67.641 | 67.625 | 125.594 | 97.266 |
| records_array_1m / scan | 158.500 | 158.500 | 97.594 | 83.172 |
| records_array_1m / roundtrip | 61.484 | 61.484 | 152.078 | 93.328 |
| records_object_1m / parse | 66.469 | 66.438 | 93.016 | 79.172 |
| records_object_1m / stringify | 63.078 | 63.078 | 129.391 | 103.688 |
| records_array_8m / parse | 108.828 | 108.828 | 265.953 | 140.844 |
| records_array_8m / stringify | 121.156 | 121.141 | 212.578 | 196.562 |
| records_array_8m / sparse | 109.156 | 109.156 | 265.969 | 186.359 |
| records_array_8m / scan | 186.719 | 186.719 | 229.906 | 133.797 |
| records_array_8m / roundtrip | 129.266 | 129.266 | 267.422 | 148.047 |
| records_object_8m / parse | 187.328 | 187.328 | 244.344 | 131.344 |
| records_object_8m / stringify | 121.188 | 121.188 | 212.578 | 196.531 |
| records_array_20m / parse | 255.422 | 255.438 | 359.859 | 220.766 |
| records_array_20m / stringify | 235.219 | 235.219 | 459.516 | 304.828 |
| records_array_20m / sparse | 255.438 | 255.438 | 359.781 | 220.656 |
| records_array_20m / scan | 255.453 | 255.453 | 330.969 | 225.641 |
| records_array_20m / roundtrip | 295.391 | 295.359 | 342.109 | 255.844 |
| records_object_20m / parse | 255.438 | 255.438 | 359.719 | 220.734 |
| records_object_20m / stringify | 235.234 | 235.250 | 459.516 | 304.953 |
| numbers_1m / parse | 62.781 | 62.781 | 104.109 | 68.219 |
| numbers_1m / stringify | 57.844 | 57.844 | 113.094 | 74.266 |
| long_string_1m / parse | 16.438 | 16.453 | 209.219 | 156.453 |
| long_string_1m / stringify | 54.484 | 54.484 | 183.641 | 151.656 |
| escaped_1m / parse | 58.406 | 58.406 | 102.547 | 67.891 |
| escaped_1m / stringify | 54.891 | 54.891 | 124.844 | 74.812 |
| unicode_1m / parse | 15.781 | 15.766 | 203.703 | 156.219 |
| unicode_1m / stringify | 53.891 | 53.875 | 186.156 | 147.719 |
| wide_1m / parse | 227.703 | 227.719 | 121.203 | 95.062 |
| wide_1m / stringify | 68.672 | 68.641 | 117.953 | 84.672 |
| heterogeneous_1m / parse | 62.562 | 62.562 | 92.875 | 99.812 |
| heterogeneous_1m / stringify | 61.719 | 61.719 | 128.891 | 104.953 |

[Timings](results/quiet-template-only-r14-full/timing.jsonl), [full-output checks](results/quiet-template-only-r14-full/verify.jsonl), [host and worker hashes](results/quiet-template-only-r14-full/host.json), [window](results/quiet-template-only-r14-full/window.json).

## Longer recheck

Window 2026-09-10T20:05:49Z–2026-09-10T20:10:43Z; one-minute load 1.199→2.342. Quiet gate passed; no competing workload detected at either boundary.

| Fixture / operation | Iterations | Main CPU | R14 CPU | Node CPU | Bun CPU | Delta | Ranges | Slower pairs |
|---|---:|---:|---:|---:|---:|---:|---|---:|
| records_array_1m / scan | 276 | 3055.681159 | 3067.347826 | 2719.847826 | 2187.481884 | +0.382% | overlap | 10/11 |
| records_object_1m / parse | 324 | 1868.268519 | 1886.481481 | 2685.922840 | 2139.462963 | +0.975% | separated slowdown | 11/11 |
| records_object_8m / parse | 40 | 18476.800000 | 18596.025000 | 32738.925000 | 20953.375000 | +0.645% | overlap | 10/11 |
| records_array_20m / parse | 16 | 48544.250000 | 48782.062500 | 89396.562500 | 52601.312500 | +0.490% | separated slowdown | 11/11 |
| records_array_20m / sparse | 16 | 48556.000000 | 48837.937500 | 88199.687500 | 52630.125000 | +0.581% | overlap | 11/11 |
| records_array_20m / scan | 16 | 49557.375000 | 49837.312500 | 86037.687500 | 53459.187500 | +0.565% | overlap | 11/11 |
| records_object_20m / parse | 16 | 48587.125000 | 48865.750000 | 88749.562500 | 52584.000000 | +0.573% | separated slowdown | 11/11 |

| Fixture / operation | Main peak RSS | R14 peak RSS | Node peak RSS | Bun peak RSS |
|---|---:|---:|---:|---:|
| records_array_1m / scan | 589.406 | 589.406 | 130.422 | 109.125 |
| records_object_1m / parse | 71.156 | 71.156 | 125.641 | 107.828 |
| records_object_8m / parse | 612.297 | 612.297 | 463.109 | 154.000 |
| records_array_20m / parse | 704.359 | 704.375 | 664.922 | 260.406 |
| records_array_20m / sparse | 704.375 | 704.375 | 664.859 | 381.359 |
| records_array_20m / scan | 704.375 | 704.375 | 529.469 | 310.844 |
| records_object_20m / parse | 704.375 | 704.359 | 664.844 | 341.250 |

[Timings](results/quiet-template-only-r14-recheck-full/timing.jsonl), [full-output checks](results/quiet-template-only-r14-recheck-full/verify.jsonl), [host and worker hashes](results/quiet-template-only-r14-recheck-full/host.json), [window](results/quiet-template-only-r14-recheck-full/window.json).

## A/A controls

Two cases were selected to investigate a possible measurement bias: 1 MiB object parsing and 20 MiB array scanning, at the same longer work counts and eleven repetitions. Both Perry arms execute the exact frozen main bytes; Node and Bun remain in the interleaved order. The first control uses the same executable path in both arms. The second uses main-worker versus control00-worker, matching the five-byte basename-length difference between main-worker and candidate-worker. Main A is the driver’s perry-labelled arm and Main B its baseline-labelled arm; neither is R14.

Same-path deltas are +0.00033% and −0.02928%, with overlapping ranges. Different-name deltas are −0.00281% and +0.00480%, also overlapping. These controls do not support timing-order or executable-name-length bias as the explanation for the candidate slowdowns. Runtime inspection confirms argv strings are allocated, but the proposed timing effect was not observed. Dedicated archives attribute only main’s build and source to these controls.

## A/A: same path and bytes

Window 2026-09-10T20:10:58Z–2026-09-10T20:12:17Z; one-minute load 1.963→2.118. Quiet gate passed; no competing workload detected at either boundary.

| Fixture / operation | Iterations | Main B CPU | Main A CPU | Node CPU | Bun CPU | Delta | Ranges | Slower pairs |
|---|---:|---:|---:|---:|---:|---:|---|---:|
| records_object_1m / parse | 324 | 1868.645062 | 1868.651235 | 2645.185185 | 2138.574074 | +0.000% | overlap | 6/11 |
| records_array_20m / scan | 16 | 49522.812500 | 49508.312500 | 87132.437500 | 53456.062500 | -0.029% | overlap | 6/11 |

| Fixture / operation | Main B peak RSS | Main A peak RSS | Node peak RSS | Bun peak RSS |
|---|---:|---:|---:|---:|
| records_object_1m / parse | 71.156 | 71.156 | 125.703 | 107.766 |
| records_array_20m / scan | 704.375 | 704.375 | 529.547 | 309.469 |

[Timings](results/quiet-template-only-r14-aa-main-full/timing.jsonl), [full-output checks](results/quiet-template-only-r14-aa-main-full/verify.jsonl), [host and worker hashes](results/quiet-template-only-r14-aa-main-full/host.json), [window](results/quiet-template-only-r14-aa-main-full/window.json).

## A/A: identical bytes, different name lengths

Window 2026-09-10T20:15:34Z–2026-09-10T20:16:54Z; one-minute load 1.780→2.043. Quiet gate passed; no competing workload detected at either boundary.

| Fixture / operation | Iterations | Main B CPU | Main A CPU | Node CPU | Bun CPU | Delta | Ranges | Slower pairs |
|---|---:|---:|---:|---:|---:|---:|---|---:|
| records_object_1m / parse | 324 | 1868.472222 | 1868.419753 | 2652.287037 | 2137.632716 | -0.003% | overlap | 5/11 |
| records_array_20m / scan | 16 | 49529.625000 | 49532.000000 | 86676.187500 | 53810.125000 | +0.005% | overlap | 4/11 |

| Fixture / operation | Main B peak RSS | Main A peak RSS | Node peak RSS | Bun peak RSS |
|---|---:|---:|---:|---:|
| records_object_1m / parse | 71.156 | 71.156 | 125.641 | 107.766 |
| records_array_20m / scan | 704.375 | 704.375 | 529.516 | 308.438 |

[Timings](results/quiet-template-only-r14-aa-path-full/timing.jsonl), [full-output checks](results/quiet-template-only-r14-aa-path-full/verify.jsonl), [host and worker hashes](results/quiet-template-only-r14-aa-path-full/host.json), [window](results/quiet-template-only-r14-aa-path-full/window.json).

## Correctness and machine code

- `RUST_TEST_THREADS=1 cargo test --release -p perry-runtime --lib json`: 292 pass. The new unit checks distinct object/array identities and mutable cache replacement immediately after a hit. The TypeScript fixture mutates earlier results and checks retained outputs across pressure.
- Exact production build: `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static`, 333.28 seconds. Compiler and both archives were frozen and hashed, with all three mtimes after build start. Four generated worker objects match main byte-for-byte and link their corresponding frozen runtime.
- Main and candidate each pass 28 Node behavioral runs across auto/tape/direct parsing and normal/scheduled/full GC, plus 14 stringify-options checks. Every scheduled run asserts positive protected page sets and moved objects. The cache fixture has 2,158 protected sets and 99,893 moved objects under auto/direct; tape has 1,999 and 95,991. Callback-only has 16 and 13,305.
- Native static checking retains seven UNSUPPRESSED findings identical to actual main: five unrooted globals, one unrooted string handle and one stale allocation value. Coverage: 83 functions, 2,226 statepoints, 8,372 relocates. All fourteen IR files match after removing only the first native ModuleID path comment; shadow IR needs no normalization. Both shadow variants pass, and ordinary/callback native controls have zero findings. No allowance was increased; this is not an all-clean static result.
- Script lint: 73/74 pass; public benchmark input freshness fails. Compile tier and two CI-only checks are skipped. The Rust file cap passes. Full CI is not claimed.

[Unit provenance](results/template-only-r14-validation/unit-source.json), [build](results/template-only-r14-validation/build-provenance.json), [main](results/template-only-r14-validation/reference-main.json), [workers](results/template-only-r14-validation/worker-object-comparison.json), [behavior](results/template-only-r14-validation/candidate-fixture-validation.json), [options](results/template-only-r14-validation/candidate-options-validation.json), [root comparison](results/template-only-r14-validation/root-comparison.json), [lint](results/template-only-r14-validation/script-lint.log.gz).

Machine code confirms removal of the whole-template memcpy and planned-array copy. The hit helper’s frame shrinks from 912 to 224 bytes, including saved registers. The remaining 64-byte external call is memset_pattern16 initializing the output-value array. parse_slow remains 512 bytes; its DirectParser still occupies sp+0xa0 and calls the same-address constructor and parse_value functions in both builds. That evidence does not support a changed parser stack-slot explanation.

[Borrow audit](results/template-only-r14-validation/borrow-audit.json), [cached code comparison](results/template-only-r14-validation/cached-machine-comparison.json), [main symbols](results/template-only-r14-validation/main-cached-entry-machine.json), [candidate symbols](results/template-only-r14-validation/candidate-cached-entry-machine.json), [parser call-site comparison](results/template-only-r14-validation/parse-slow-construction-call.json), [A/A verifier](results/template-only-r14-validation/analyze-aa.py), [different-name A/A verifier](results/template-only-r14-validation/analyze-aa-path.py).

## Remaining scope and next experiment

All 24 lazy-array probes preserve main’s outcomes and complete stdout: twelve two-record cases pass Node; at 180 records, six zero/true spacing cases crash with SIGSEGV and two plain whitespace/duplicate-key cases return noncanonical raw JSON. The other four pass. The separate main/Bun versus Node fractional-spacing gap is unchanged. These baseline failures are not conformance passes.

[Lazy main](results/template-only-r14-validation/lazy-main-probes.json), [lazy candidate](results/template-only-r14-validation/lazy-candidate-probes.json), [fraction baseline](results/template-only-r14-validation/fraction-baseline.json), [fraction candidate](results/template-only-r14-validation/candidate-fraction.json).

The next minimal experiment should keep template borrowing within the original out-of-line reuse function, explicitly preventing its new admission code from being inlined into parse_slow. It should remove the predicate/construction split while preserving the existing collection and borrow ordering. This tests the changed call boundary; it does not assume it caused the slowdown.

A separate later investigation is to construct captured templates directly in their final local struct. Read-only main disassembly shows a 576-byte local-values-to-entry copy followed by a 600-byte entry-to-cache copy, plus copied array plans for root-barrier iteration. Those copies are not removed in R14 and are not proven causes of its regression.

[Capture code](results/template-only-r14-validation/main-template-capture-machine.json), [capture follow-up](results/template-only-r14-validation/capture-followup.md).

Access, extended short-call, stringify-options timings, broader rotating coverage and the 36 retained-output cases were not run after full-matrix qualification failed. Prepared drivers are not evidence of execution. The full objective remains open. R14 is parked without a PR or release version bump.

[Rotating verifier](results/template-only-r14-validation/analyze-rotating-retry1.py), [full verifier](results/template-only-r14-validation/analyze-full.py), [recheck verifier](results/template-only-r14-validation/analyze-full-recheck.py), [rotating vectors](results/template-only-r14-validation/rotating-retry1-analysis.json), [full vectors](results/template-only-r14-validation/full-analysis.json), [recheck vectors](results/template-only-r14-validation/full-recheck-analysis.json), [A/A vectors](results/template-only-r14-validation/aa-analysis.json), [different-name A/A vectors](results/template-only-r14-validation/aa-path-analysis.json), [validation manifest](results/template-only-r14-validation/manifest.json). Initial staging verified 102 hashes; different-name control staging verified 103 including the identical-main copy.

[R13 report](https://github.com/PerryTS/perry/blob/cc7eca082c2b075f6a0cb8f4760af0d5b63817f8/benchmarks/json_performance/BORROWED_TEMPLATE_R13.md) records the preceding combined experiment. Earlier accepted work landed through [merge train #10037](https://github.com/PerryTS/perry/pull/10037).
