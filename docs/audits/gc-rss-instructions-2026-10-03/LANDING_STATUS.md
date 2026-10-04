The adopted GC source is 0aa483aa91ab901583002ec0174a50884b60ad81, measured against main ad0a2617bf0b9708032cf89c86575e9bd69fb074. The landing tree rebases those changes onto main 0a5bb47dc5f10dcfecd8c14fe4b3fc2e7ac4703c and converts two stream-cache declarations to fast TLS. Measurements below remain bound to the measured commits, not the subsequent landing commit.

V79: 17 workloads, 204 correct executions, three RSS and three instruction samples per workload and arm. All RSS medians improved except unchanged noop. TypeScript RSS -4.741%, instructions +0.409%; trees RSS -11.665%, instructions -0.404%; Zod RSS -0.264%, instructions -0.047%. Complete signed results and independent verification accompany this file.

V80 Linux and macOS: 15 event tests, 75 FFI tests, and doc tests passed. Runtime: Linux 4867 passed, macOS 4881 passed, five ignored on each. Moving/protected linked coverage: 22 executions, 11 with actual moving collections.

Excluded from landing: pending-mark-frontier, RSS-query sampling, learned nursery floor, native prefix, object-header or shape changes. Class-value scanning belongs to Claude; subsequent Codex work covers pacing, conservative scanning #11873 and incremental correctness #11842.
