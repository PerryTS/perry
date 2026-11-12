The `net transport sabotage` runtime test no longer races the kernel: witness
n9 drives the stale resolver's completion through the test-only replay helpers
instead of waiting for localhost's resolver dispatch, so the `plan_check`
sabotage cannot miss its window and the witness is deterministic. No production
change; test-only.
