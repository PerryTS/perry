# Opening scan R3: expanded stringify comparison

Quiet M1/8 GiB window 2026-09-09 21:11:01–21:13:38 UTC. Two large-string fixtures, 32768 calls with eight warmups per trial, 27 repetitions and three immutable Perry arms: candidate R3, freshly built main eee3881c4, and older corrected-R2 prior. Node supplies the output oracle. All 162 timing and eight output-oracle processes pass the runner checks.

| Fixture | Arm vs fresh main | Median CPU change | Mean CPU change | Median paired change | Slower pairs |
|---|---|---:|---:|---:|---:|
| long_string_1m | perry | +2.510% | +1.269% | +0.907% | 18/27 |
| long_string_1m | prior | -0.705% | +0.492% | +0.268% | 14/27 |
| unicode_1m | perry | +2.642% | +0.541% | +2.875% | 14/27 |
| unicode_1m | prior | +1.742% | +0.489% | -1.510% | 12/27 |

The candidate still has higher large-string medians, but mean shifts are smaller and the prior control also varies against fresh main. These are descriptive comparisons, not proof of a fixed overhead or equal performance. Maximum candidate median peak-RSS increase is 160 KiB. The sparse/heterogeneous concerns from the separate nine-repetition replay remain unresolved, so R3 stays experimental.

[All samples and descriptive comparisons](reference-screen.json), [raw timing](timing.jsonl), [quiet admission](window.json), and exact runners/provenances are archived. Candidate and prior labels are identified by immutable binary hashes in host.json. No rows or repetitions were discarded.
