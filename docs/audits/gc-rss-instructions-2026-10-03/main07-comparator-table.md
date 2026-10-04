## Latest compiler comparison

Perry now uses the verified **main 07e50b14a3 GC proposal**. Seven unchanged standalone workloads have identical successful outputs, independently checked against their raw binaries/RSS logs. Values are median peak RSS **MiB**, three correct repetitions per engine on the primary Linux host, measured at different times. Official comparator refs were last checked **2026-10-04 00:37Z**: ScriptC 0.2.2, Porffor alpha15/main72d048d, Static Hermes static_h/ade4a2b.

| Workload | Perry GC | ScriptC 0.2.2 | Porffor alpha 15 | Static Hermes |
|---|---:|---:|---:|---:|
| 00-noop | 1.26 | 1.66 | 1.54 | 9.64 |
| 15-crc32 | 15.72 | 9.67 | 10.06 | 17.53 |
| 23-binary-trees | 178.11 | 585.54 | 400.97 | 253.10 |
| 30-string-build | 210.62 | 183.10 | 339.26 | 301.95 |
| 31-json | 153.76 | compile failed | 166.10 | 110.11 |
| 51-pipeline | 254.45 | 261.29 | 403.38 | 386.49 |
| 60-ring-churn | 14.39 | 1.80 | 37.33 | 14.28 |

[Source/raw-output/binary/RSS verification](https://github.com/PerryTS/perry/blob/rss-gc-runtime-20261003/docs/audits/gc-rss-instructions-2026-10-03/main07-comparator-join.json). ScriptC JSON compilation fails and has no RSS rank. Its first real TS/Zod default/static-npm attempts stop at the wrappers' implicit-any checker error; this does not establish native support failure. A separate probe keeps source bytes and dependencies identical and accepts implicit-any in project checker configuration. No dynamic-engine fallback or source/library stubs are used.

