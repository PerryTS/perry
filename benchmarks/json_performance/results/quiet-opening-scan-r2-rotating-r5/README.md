# Opening scan R2: complete changing-input suite

Quiet M1/8 GiB window 2026-09-09 20:20:17–20:28:36 UTC. All 380 output comparisons and 1140 timing trials passed: 19 fixtures, three modes, four engines and five repetitions. Reference is the immutable pre-merge corrected R2 build from #10034; its reviewed source tree is now main through #10035. This run is not relabelled as using a fresh post-merge build.

Eight equal-size/shape inputs remain live in all modes. Seventeen fixtures vary one value; null and empty-object fixtures have identical separately loaded text. Same-source and selection-only controls are separate and selection time is not subtracted.

| Fixture | CPU change vs corrected R2 | Ranges | Perry / best Node or Bun |
|---|---:|---|---:|
| null | -2.008% | separated faster | 0.775× |
| string_a | -0.806% | separated faster | 0.855× |
| empty_object | -1.844% | separated faster | 1.171× |
| tiny_object | -0.367% | overlap | 0.997× |
| small_record | -0.157% | separated faster | 1.950× |
| object_1k | -3.350% | separated faster | 1.999× |
| records_array_16k | +0.018% | overlap | 0.542× |
| records_array_1m | +0.013% | overlap | 0.563× |
| records_object_1m | -1.371% | separated faster | 0.928× |
| records_array_8m | -0.125% | overlap | 0.540× |
| records_object_8m | -1.295% | separated faster | 0.763× |
| records_array_20m | -1.788% | separated faster | 0.896× |
| records_object_20m | -2.027% | separated faster | 0.895× |
| numbers_1m | -0.239% | overlap | 0.500× |
| long_string_1m | -7.556% | separated faster | 1.429× |
| escaped_1m | -0.795% | separated faster | 0.586× |
| unicode_1m | -4.676% | separated faster | 2.259× |
| wide_1m | +0.473% | separated slower | 0.649× |
| heterogeneous_1m | +0.303% | overlap | 0.483× |

R2 repairs R1’s tiny/empty regressions and retains the large-string gains: ASCII 7.556% faster and Unicode 4.676% faster. However, rotating wide-object parse is 0.473% slower with separated ranges. Together with the full original sparse/heterogeneous regressions, this keeps R2 experimental. R3 outlines the wide search tail while keeping the first probe inline; it is a separate candidate under validation.

Maximum median peak/current RSS increases across all modes are 128/160 KiB. The much larger absolute container-RSS gaps to Node/Bun remain; no heap-retention improvement is claimed.

[Every CPU and RSS row](comparison.md), [paired samples](reference-screen.json), [quiet admission](window.json), raw JSONL, exact runners, source patch, artifact hashes and GC witnesses are archived together.
