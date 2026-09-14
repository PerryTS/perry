# Local validation replay

Baseline: main `eb13fa188d0690fb964cd6df329593e05aa55617`, version 0.5.1564.
This commit already contains the #10215 large-array fix. All edits and builds
used session-owned worktrees and targets; the user's dirty checkout was not
modified. The feature branch does not bump the workspace version. Landing
requires the repository's merge train and a replay on the assembled train.

## Runtime and static gates

The final source, including the constructor-identity guard, was checked on
macOS arm64 with nightly-2026-08-20. Default runtime features include both
regex-engine and dyn-eval, so all 13 registered restore witnesses run.

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | Pass |
| `cargo check -p perry-runtime --no-default-features --features full` | Pass |
| `cargo test -p perry-runtime --lib -- --test-threads=1` | 3,811 passed, 4 ignored; baseline 3,798 passed, 4 ignored |
| One omitted production restore at a time | All 13 detected at the nested enclosing-state assertion; restored exception suite: 20 passed |
| `cargo clippy -p perry-runtime --lib --tests --message-format=json` compared with main | No added or removed diagnostics; both return 101 for the same existing errors |
| `scripts/run_lint_gates.sh` | 80/83 pass; three baseline-reproduced failures, two CI-only skips |

Clippy diagnostics are compared as multisets of level, lint code, message and
primary filename, ignoring source-line movement. The baseline has 12
`clippy::approx_constant` errors; `clippy-comparison.json` records the diff.
The user-listed `native_stack` test passes on this host.

The final complete lint replay ran 83 gates: 80 passed and three failed,
with two CI-only checks skipped by the script. Each failure was independently
reproduced on the clean baseline:

- Public benchmark evidence freshness is stale on main.
- Host-compatible all-targets `RUSTFLAGS="-D warnings" cargo check` rejects
  unused WebAssembly helpers in unchanged `global_this_webassembly.rs`.
- Generated API documentation differs from committed docs. Baseline and
  candidate generated Markdown and declarations are byte-identical, including
  missing committed bun-pty entries. Generated documentation was restored
  after checking; no baseline or warning suppression is included in the patch.

Product strict-warning, product clippy and workspace clippy gates pass.
The full lint transcript and baseline comparisons are retained beside this
file. The GC custody, root inventory and wiring checks are part of that replay.

## GC replay

The catch-frame patch before the seven-line constructor marker guard passed
the full Linux matrix: 86 corpus fixtures × 22 arms = 1,892 cells, with 1,533
PASS, 359 UNVER, zero XFAIL and zero FAIL. Every one of the 21 arms with a
liveness requirement exercised its required collector operation. The shipped
control has no liveness requirement. The 359 UNVER cells are not counted as
passing checks; they are retained by the official matrix harness.

This matrix used a coherent release build of `perry`, `perry-runtime-static`,
`perry-stdlib-static`, `perry-ext-net`, and `perry-ext-http`, pinned Node 26.5.1,
`PERRY_NO_AUTO_OPTIMIZE=1`, and the full runtime archives. The explicit
extension packages avoid mixing archives with a different runtime source
stamp. GC representation flags still vary across the matrix's compile groups.
The full JSON report is `gc-stress.json.gz`. The final guarded PR-arm replay also passes: 86 fixtures × seven arms =
602/602 byte-exact Node matches, 431 PASS, 171 UNVER, zero XFAIL and zero
FAIL. All six required liveness checks pass. Its complete report is
`guard-gc-stress.json.gz`, with liveness counters in
`guard-gc-stress-summary.txt`.

The final guarded Linux ratchet ran seven repetitions of all 14 probes against Node 26.5.1.
Its same-host baseline was built from the stated main commit. All 126
heap/collector medians match exactly and `check --profile shared_ci` passes
against that fresh comparison artifact. The artifact is evidence only; the
repository's pinned baseline is unchanged.

The exact pinned ratchet was also replayed on macOS with the workflow's
three-package release graph and default one-codegen-unit release profile.
The GC harness unit tests and `validate --scope structural` pass. Baseline and
final guarded candidate have identical values for all 126 heap/collector
medians; both fail the pinned artifact at the same 30 cells. The ordered
failure diagnostics are identical (`pinned-gc-comparison.json`). Wall time and RSS are observations only, not performance claims.

## Compiled integration

The final Linux replay completed all four disjoint
`run_parity_tests.sh --shard N/4` shards with the frozen guarded three-package
artifacts used for instruction measurements. Auto-optimized extension
variants requested by the harness used the matching guarded source tree.
The official merge required all four reports and verified disjoint test IDs.

| Complete inventory | Result |
|---|---|
| All parity fixtures | 1,633 entries: 1,472 pass, 93 output mismatches, 29 compile failures, two timeouts classified as crashes, five Node oracle failures, 32 skips |
| Gap subset | All 785 entries: 776 pass, nine output mismatches, no crashes/compile failures/oracle failures/skips |
| Aggregate threshold | Pass, 93.9% by the harness's formula |
| Matrix trend | Pass |
| Gap snapshot | Three baseline-reproduced mismatches |
| Platform known-failure gate | Eight unlisted results: seven reproduce on main; the eighth passes an isolated candidate replay after restoring its echo-server prerequisite |

Every `test_gap_` fixture ran as part of the full sweep. The extracted gap
report was checked with the same `gap_snapshot.py check` used by
`scripts/run_gap_tests.sh`. Both complete reports, coverage assertions and
all gate commands/results are retained. No allowlist or snapshot is relaxed.

The seven unlisted baseline-reproduced results are:

- `test_bun_plugin` and `test_dynamic_import_data_10104` (oracle/import behavior).
- `test_gap_2899_2779_2777_static_helpers`, `test_gap_disposablestack_2875`, and
  `test_gap_iterator_prototype_next_patch` (also the three gap-gate failures).
- `test_issue_10155_textfield_singleline` (compile failure).
- `test_issue_336_class_keys_collision` (Node rejects namespace syntax).

The eighth, `test_sock_write_map`, encountered a missing echo server during
the parallel sweep: both Node and Perry received connection-refused errors.
The official isolated fixture passes on both main and the guarded candidate.
`socket-recheck.md` records the cause and successful replay; the original
aggregate report is retained without rewriting its failure to a pass.

Both crashes are 10-second timeouts and reproduce on main:
`test_issue_4975_http_agent_keep_alive_connection_options` and
`test_parity_stream_web`. Their existing failure records cite closed #8841
and #793 respectively; that stale tracking is not presented as a new open
issue. See `additional-baseline-checks.json` for the observed behavior.

The five Node oracle failures are long-lived server/channel fixtures:
`test_node_http_basic`, `test_issue_4826_array_headers`,
`test_node_http_post_body`, `test_message_channel_global`, and
`test_node_http_ws_upgrade`. Perry was not tested by those entries. The array
headers oracle timeout was also reproduced with the clean baseline harness.
The test sources and harness are unchanged by this PR. These entries and the
32 skips remain explicit gaps in this replay's execution coverage.

Constructor-marker regression and guard-omission evidence are recorded in
`constructor-identity.md`. All five targeted guarded rechecks pass, and all
five also pass in the complete sweep.

## Evidence provenance

`instructions.json` records the SHA-256 of the reviewed
`git diff eb13fa188d -- crates` patch and the frozen three-package artifacts.
`source-files.json` hashes all 20 changed or added Rust source files; each
matches the Linux validation source byte for byte. Final instruction
measurements and full integration include the constructor guard. The older
full 22-arm matrix precedes only that guard; its scope is stated above. The
final source passes the seven-arm PR matrix and matches main on both GC
ratchet measurements.

Full logs and binaries are retained under `/root/js-throw-evidence` on
perrymaster. Local unit, lint, clippy and pinned GC logs are under
`/tmp/perry-catch-evidence`. Repository evidence contains compact JSON, raw
instruction-stat outputs, folded stacks, fault transcripts and gate reports.
Superseded incomplete integration runs, including an early source-stamp
mismatch, are excluded from the final results.
