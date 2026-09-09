# Opening scan R3: changing-input focus

Quiet M1/8 GiB window 2026-09-09 20:40:25–20:42:57 UTC. Six fixtures, three modes, four engines and nine repetitions; all 120 output comparisons and 648 timing trials passed. Reference is the pre-merge corrected R2 worker. Eight input members remain live, with one varying value per fixture; same-source and selection controls are separate and selection CPU is never subtracted.

| Fixture | Changing-input CPU change | Sample ranges |
|---|---:|---|
| small_record | -0.284% | overlap |
| object_1k | -3.376% | separated faster |
| long_string_1m | -7.540% | separated faster |
| unicode_1m | -4.549% | separated faster |
| wide_1m | +0.097% | overlap |
| heterogeneous_1m | -0.087% | overlap |

No row in this focus has separated slower ranges. Wide-object and small-record results overlap reference; large-string and 1 KB object gains remain separated. Maximum median peak/current RSS increases are 112/144 KiB. Full original and changing-input matrices against a freshly built merged main remain necessary before accepting the candidate.

Exact runners, raw trials, window metadata, source patch, immutable hashes and GC witnesses are preserved in this directory.
