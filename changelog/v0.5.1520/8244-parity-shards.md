### `parity` sharded 8 ways + aggregate fan-in (#8244)

The first full-tier run of the tiered CI (#8187) proved the unsharded `parity`
job cannot complete: GitHub's 6-hour job cap killed it (run 31935729773). It is
now a plan-driven 8-shard matrix (`run_parity_tests.sh --shard N/8`), with
`parity_known_failures.py` running per shard (shard-safe by design) and a new
`parity-aggregate` fan-in that merges the shard reports via
`scripts/parity_report_merge.py` (self-tested; refuses a missing shard rather
than shrinking the suite) and runs the aggregate-only gates: the threshold
minimums and the per-module matrix trend. `full-suite-gate` — the job
`release-packages.yml`'s `await-tests` waits for — requires the aggregate, so
the release gate can now actually finish.
