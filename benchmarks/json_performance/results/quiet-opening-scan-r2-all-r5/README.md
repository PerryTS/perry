# Opening scan R2: full original matrix

Quiet M1/8 GiB window 2026-09-09 20:12:11–20:17:16 UTC. Four engines and five
repetitions; all 200 correctness checks and 344 measurement groups passed.
The candidate includes the widened opening scan and outlined empty allocator.
Reference is corrected R2 from #10034; that exact reviewed tree landed on main
through #10035 as `eee3881c464bf91ae900e87a42bed072bbdfc95a` during this run.
The timing artifact remains the immutable pre-merge corrected-R2 build, not a
fresh post-merge build, and is not relabelled here.

Candidate remains ahead of the better Node/Bun median on 46/50 original CPU
rows, 67/86 peak-RSS rows and 31/36 retained-current-RSS rows. Existing full-scan,
large roundtrip and memory gaps remain. Original timing repeats one source;
changing-input parsing is evaluated in the separate rotating suite.

This is not a regression-free result. Two timing rows have separated slower
candidate ranges:

| Fixture / operation | CPU change |
|---|---:|
| heterogeneous_1m / parse | +0.467% |
| records_array_16k / sparse | +0.614% |

Maximum median peak/current RSS increases are 192/144 KiB. The sparse-row
slowdown also appeared in the longer paired replay, so it remains an open
investigation rather than being dismissed as measurement noise.

[All target comparisons](parity.md), [all candidate/reference samples](reference-screen.json),
[quiet-host admission](window.json), and exact raw trials, runners, hashes,
source patch and GC witnesses are archived here.
