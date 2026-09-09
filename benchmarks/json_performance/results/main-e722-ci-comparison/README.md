# Pristine Linux main reproduces the correction PR CI failures

[Pristine main run 34398426676](https://github.com/PerryTS/perry/actions/runs/34398426676)
built `e7223f700c8ce69c388210dab394ad7142550527` with tier `pr` and
`update_gap_snapshot=false`. It completed with failure. All seven named gap
regressions from [PR10034 run 34392345218](https://github.com/PerryTS/perry/actions/runs/34392345218)
reproduce on this pristine base, as recorded in comparison.json. Excerpts retain
original log line numbers and context; complete logs remain in the linked run.

The same native_stack::tests::stack_top_respects_custom_thread_stack_sizes
failure occurs on pristine main (3494 passed, 1 failed, 4 ignored) and PR10034
(3498 passed, 1 failed). The lint public-baseline freshness failure was separately
shown to have identical source fingerprints on both trees. GC stress and gap
shards 4/6 pass. No snapshot, test expectation or required gate was changed.

This comparison classifies the earlier correction's failures. It does not
substitute for CI on the opening-scan PR or authorize bypassing pr-gate.
