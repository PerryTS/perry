The seven standalone workloads below have identical TypeScript source hashes and output across successful engines. Values are peak RSS medians in MiB from three correct executions on the primary Linux measurement host, taken at different times. Perry is the GC proposal measured on **main2026ecfe**, rather than the newer main07 checkpoint still under application validation. ScriptC is **0.2.2**, Porffor is **alpha15 /72d048d**, and Static Hermes is **static_h /ade4a2b**. Official npm/Git refs were checked again at **2026-10-04 00:37Z**.

| Workload | Perry GC | ScriptC 0.2.2 | Porffor alpha 15 | Static Hermes |
|---|---:|---:|---:|---:|
| 00-noop | 1.26 | 1.66 | 1.54 | 9.64 |
| 15-crc32 | 15.89 | 9.67 | 10.06 | 17.53 |
| 23-binary-trees | 178.44 | 585.54 | 400.97 | 253.10 |
| 30-string-build | 210.68 | 183.10 | 339.26 | 301.95 |
| 31-json | 153.80 | compile failed | 166.10 | 110.11 |
| 51-pipeline | 254.38 | 261.29 | 403.38 | 386.49 |
| 60-ring-churn | 14.44 | 1.80 | 37.33 | 14.28 |

Source, compiler/binary hashes, raw stdout, RSS logs, stripped Hermes input and summary bindings were independently verified. All 217 evidence files were copied locally and rehashed. ScriptC's JSON compilation failure has no RSS rank. These standalone results do not establish cross-engine support or memory use for the real TypeScript/Zod applications. A separate ScriptC0.2.2 probe is queued against those unchanged applications in default and explicit static-npm modes, without a dynamic engine or library stubs.
