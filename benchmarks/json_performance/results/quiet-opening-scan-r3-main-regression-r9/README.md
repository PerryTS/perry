# Opening scan R3: longer replay against fresh merged main

Quiet M1/8 GiB window 2026-09-09 21:05:14–21:06:35 UTC. Seven explicit cases, nine repetitions, two immutable Perry arms, 126 timing processes and 21 output-oracle processes. Node supplies the output oracle. Candidate/reference hashes, exact runners and the finished quiet window are archived. Reference is freshly built main `eee3881c464bf91ae900e87a42bed072bbdfc95a` (0.5.1529).

| Fixture | Operation | Calls per trial | CPU median change | Ranges | Slower paired repetitions |
|---|---|---:|---:|---|---:|
| records_array_16k | sparse | 20000 | +0.242% | overlap | 8/9 |
| heterogeneous_1m | parse | 512 | +0.197% | overlap | 8/9 |
| records_array_1m | parse | 512 | +0.217% | overlap | 8/9 |
| wide_1m | parse | 128 | -0.157% | overlap | 2/9 |
| small_record | stringify | 5000000 | +0.105% | overlap | 5/9 |
| long_string_1m | stringify | 32768 | +4.458% | overlap | 5/9 |
| unicode_1m | stringify | 32768 | +1.903% | overlap | 6/9 |

All sample ranges overlap, but the sparse median remains +0.243%, and every one of its nine paired repetitions is slower. Heterogeneous parse remains +0.197%, with eight of nine pairs slower. This repeats the smaller-array concerns from previous matrices and is not accepted as regression-free.

Large ASCII/Unicode stringify medians remain +4.46%/+1.90%, with variable samples. Their instruction counts are nearly identical between arms; that does not establish equal CPU cost. Maximum median peak-RSS increase is 192 KiB. PR #10036 stays draft while these observations are investigated.

[All samples](reference-screen.json), [raw timing](timing.jsonl), [quiet admission](window.json). No timing or memory rows were discarded.
