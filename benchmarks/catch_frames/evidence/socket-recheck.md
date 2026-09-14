# Socket fixture isolation

The complete four-shard report retains `test_sock_write_map: parity_fail`.
Its Node output was `ERROR: Error: connect ECONNREFUSED 127.0.0.1:17891`;
Perry printed `ERROR: [object Object]`. The required echo service was absent.

`run_parity_tests.sh` starts an echo server on the same fixed port in every
shard. Only one process can own that port, while every shard kills its own
helper on exit. The owner shard completed before this late fixture. This
explains the lost prerequisite; it is not evidence of broken socket writes.

The official filtered fixture passes in isolation on both the frozen main
and final guarded builds, with the harness starting a live echo server.
The final guarded Node/Perry stdout is byte-identical and retained in
`socket-isolated-output.txt`; its report is `socket-isolated-recheck.json`.
The baseline report is included in `baseline-rechecks.json.gz`.

The original full-suite report is unchanged. No allowlist entry, snapshot
change, harness patch or replacement aggregate result is used to hide this
infrastructure failure. The separate replay adjudicates this one result.
