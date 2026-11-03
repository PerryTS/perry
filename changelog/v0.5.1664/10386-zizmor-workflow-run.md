Unbreak the `zizmor` gate, red on `main` since 2026-09-06 on one high finding:
`dangerous-triggers` against `gate-failure-watch.yml`'s `workflow_run`.

zizmor flags that trigger categorically ("almost always used insecurely", at
Medium audit confidence). Both insecure uses are already closed in the file: the
`observe` job's `if:` admits only schedule, or dispatch/push on `main` or a `v*`
tag, so a fork PR's run can never reach the write-capable token; and the
checkout pins `ref: main` with `persist-credentials: false` and executes only
the default-branch `scripts/gate_failure_watch.py`, so the triggering run's code
is never run and none of its artifacts are downloaded.

Suppressed inline rather than in `.github/zizmor.yml`, so the justification sits
beside the trigger it excuses and carries its own ratchet: an artifact download,
a `head_sha` checkout, or a looser `if:` means deleting the marker and letting
the gate fail again.

Verified against the pinned zizmor 1.28.0 with the workflow's own invocation —
`zizmor .github/ --min-severity high` goes from exit 14 with one high finding to
exit 0. The marker must trail the `on:` key; the identical text as a comment
block above it suppresses nothing.
