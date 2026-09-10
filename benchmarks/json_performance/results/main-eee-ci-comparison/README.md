# Current main and PR CI comparison

PR [run 34447665074](https://github.com/PerryTS/perry/actions/runs/34447665074)
completed with failure at a1b39840283ce7c771a776706621a9ca249a8e8e.
Current main [run 34440726868](https://github.com/PerryTS/perry/actions/runs/34440726868)
also failed at eee3881c464bf91ae900e87a42bed072bbdfc95a.

All 748 gap-suite artifact statuses match by test name across main's three and
the PR's six shards: 736 passes, 11 parity failures and one crash/timeout. The
per-shard reports and complete sorted nonpass comparison are retained. Job-level
success can include known failing test statuses; artifact comparison covers all
six PR shards, including successful jobs. These artifacts contain statuses, not
full stdout/stderr differences, so identical failure output is not asserted.
This complete comparison supersedes earlier partial counts from job logs.

Both runs fail public-baseline freshness and the Linux
`native_stack::tests::stack_top_respects_custom_thread_stack_sizes` assertion.
PR runtime tests report 3504 passed, 1 failed, 4 ignored; main reports
3498 passed, 1 failed, 4 ignored. The normalized public source and harness hashes
are identical on these two commits; the recorded source fingerprint is stale
on both. Raw logs and the read-only Git-blob fingerprint comparison are retained.

On this PR, GC stress build, matrix and aggregate passed. Warnings, check,
scoped e2e and security checks also passed. The PR remains ready for review and
blocked by required checks. Matching baseline failures do not authorize bypass.
The failed local public refresh is separately recorded in
[public-refresh-local-busy-a1b](../public-refresh-local-busy-a1b/README.md).
No post-merge measurement exists because this PR has not merged.
