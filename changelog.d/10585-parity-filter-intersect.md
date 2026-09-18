Fixes #10585.

**Repeated `--filter` on `run_parity_tests.sh` now INTERSECTS instead of last-wins.**

`scripts/run_gap_tests.sh` is a wrapper that runs `run_parity_tests.sh --filter test_gap_ "$@"`,
so `run_gap_tests.sh --filter buffer` reached the harness as `--filter test_gap_ --filter buffer`.
The argument loop assigned `TEST_FILTER="$2"` per occurrence, so the last one won and the
wrapper's own `test_gap_` was dropped on the floor: the "gap suite" then ran every one of the
1672 fixtures whose name contains `buffer`, not the 823 `test_gap_*` fixtures that `pr-gate`'s
gap shards select. A caller could not narrow the gap suite — only silently replace it.

Measured on a merge-queue validation sweep: of 192 fixtures run across seven area filters,
**94 (21m08s of 41m37s) were outside the gate's scope**, and six of the seven reds were in
that half — reds that cannot block a merge but cost a full A/B each to attribute.

`TEST_FILTERS` is now an array and a fixture must match every entry; `TEST_FILTER` keeps the
joined form for the journal key, the wasm special case and messages, and `--resume` re-emits one
`--filter` per entry. A single `--filter` and the no-filter full sweep are unchanged, and no
caller in `.github/workflows/` or `scripts/` passes two filters to this script. The array
expansions use `${TEST_FILTERS[@]+"${TEST_FILTERS[@]}"}`, since macOS ships bash 3.2 where
`"${arr[@]}"` on an empty array is an unbound-variable error should the script ever `set -u`.
