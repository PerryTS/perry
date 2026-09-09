# Opening scan R3 versus fresh merged main: changing inputs

Quiet M1/8 GiB window 2026-09-09 20:56:08–21:04:27 UTC. All 380 output checks and 1140 timing trials pass: 19 fixtures, three modes, four engines and five repetitions. Candidate is the immutable R3 build; baseline is freshly built main `eee3881c464bf91ae900e87a42bed072bbdfc95a` (0.5.1529). Both provenances are archived.

Every engine keeps eight equal-size, same-shape inputs live. Seventeen fixtures change one value; null/empty keep identical contents in separately loaded strings. Same-source and selection-only controls use that same pool. Selection overhead is reported without subtraction. Arrays may defer materialization; full-scan results are in the original suite.

No parse row in either rotating or same mode has separated slower candidate/reference ranges. All changing-input CPU median deltas follow (negative is faster):

| Fixture | CPU change vs main | Sample ranges |
|---|---:|---|
| null | -1.889% | separated faster |
| string_a | -0.753% | separated faster |
| empty_object | -2.141% | separated faster |
| tiny_object | -0.451% | separated faster |
| small_record | -0.254% | overlap |
| object_1k | -2.759% | separated faster |
| records_array_16k | -0.099% | overlap |
| records_array_1m | -0.006% | overlap |
| records_object_1m | -1.443% | separated faster |
| records_array_8m | -0.168% | overlap |
| records_object_8m | -1.838% | separated faster |
| records_array_20m | -2.034% | separated faster |
| records_object_20m | -1.829% | separated faster |
| numbers_1m | -0.245% | separated faster |
| long_string_1m | -7.529% | separated faster |
| escaped_1m | -0.724% | separated faster |
| unicode_1m | -4.670% | separated faster |
| wide_1m | +0.131% | overlap |
| heterogeneous_1m | -0.039% | overlap |

Two selection-only controls have separated slower ranges: string_a +0.403%, object_1k +0.260%. These run no JSON operation and are retained as controls, not subtracted from the parse rows. Maximum median peak/current RSS increases across all modes are 112/144 KiB.

[All engine CPU/RSS medians and controls](comparison.md), [every reference sample](reference-screen.json), [quiet admission](window.json), and exact runners/raw trials are archived here. The [longer original-suite replay](../quiet-opening-scan-r3-main-regression-r9/README.md) retains the sparse/stringify concerns; this suite alone does not establish absence of regressions.
