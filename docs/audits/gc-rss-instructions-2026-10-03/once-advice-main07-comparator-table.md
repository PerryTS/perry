# Adopted once-advice draft versus other compilers

Perry source-equivalent to 8e3013733d on main07 (draft runtime commit 4401b3b981). Three correct peak-RSS samples per engine, identical source/output. Measurements taken at different times on the same Linux host. Official refs last checked 2026-10-04 00:37Z: ScriptC 0.2.2; Porffor alpha15/main72d048d; Static Hermes static_h/ade4a2b. Values are MiB. Full distributions/raw bindings are in the join JSON. The earlier main07 comparison table retains its original preceding-GC measurements.

| Workload | Perry draft | ScriptC 0.2.2 | Porffor | Static Hermes |
|---|---:|---:|---:|---:|
| 00-noop | 1.19 | 1.66 | 1.54 | 9.64 |
| 15-crc32 | 15.66 | 9.67 | 10.06 | 17.53 |
| 23-binary-trees | 157.32 | 585.54 | 400.97 | 253.10 |
| 30-string-build | 210.62 | 183.10 | 339.26 | 301.95 |
| 31-json | 153.62 | compile failed | 166.10 | 110.11 |
| 51-pipeline | 254.24 | 261.29 | 403.38 | 386.49 |
| 60-ring-churn | 14.27 | 1.80 | 37.33 | 14.28 |

No real TypeScript/Zod cross-engine equivalence is claimed. ScriptC native real-app attempts fail lowering/type restrictions and receive no RSS rank.
