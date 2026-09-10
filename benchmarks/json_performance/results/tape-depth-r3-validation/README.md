# Tape-depth R3 validated-source guard experiment

Source/build `2e5989b5e7b6d7e162de0120565f3f9c58059747`, version 0.5.1530.
R3 checks an already-validated direct source before the new preflight-route
guard. Eligible arrays now perform that TLS read too, so its cost needs
measurement. Collector policy and construction semantics are unchanged.

All 291 JSON release tests pass single-threaded (4m39s build, 1.60s tests).
The matched compiler/runtime-static/stdlib-static release build passes in
5m22s with fixed HEAD and source hashes. All nine compiled depth/GC combinations
match Node; each scheduled arm protects 2772 retired sets and moves more than
46000 objects. Existing protected scan: 1323 minors/208531 moved. Retained
outputs, full-GC behavior, fourteen escaped-record comparisons and twelve
unfixed-main negative failures remain covered. Stringify malloc counts are
39,40,40,40 in candidate and main. Raw/address/root/store/fmt/file-size gates
pass; human-audited store classes are not claimed as machine-verified.

The two builders retain their addresses and 10396/10764-byte spans. Their
non-call instructions are identical; each has 25 relocated calls to the same
named callees. Callee bodies were not compared, so neither byte identity nor
whole-program equivalence is claimed. The comparison asserts full instruction
coverage of both spans and records every changed call. An initial strict byte
comparison rejected identity; the narrower claim reflects those differences.
`parse_slow` changes from 5624 to 5668 bytes at the same start address, so the
guard change survives optimization.

The [focused replay](../quiet-tape-depth-r3-main-focus-r9-r27/README.md) passes
52 output checks and 405 timing trials, but does not resolve cached Unicode
parse (+0.220% versus main) and has heterogeneous stringify +1.804% versus main
and +1.253% versus R2, with all nine paired repetitions slower than main. R3 is
parked without full-matrix acceptance. Its source is not part of PR #10036;
R2 remains the PR source. Exact tests/build/GC logs, outputs, source hashes, drivers and assembly
are archived. Local driver paths are historical replay records.
