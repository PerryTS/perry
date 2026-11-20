Fix `scripts/native_result_ledger.py`, which was red on `main` and blocking
the path-filtered `Native Result Ledger` workflow on every PR touching
`crates/perry-codegen/src/lower_call/native_table/**` or the ledger files.

Two defects, one masking the other. `EXPECTED_ROWS` was stale at 371 while
#10658's `net.Socket` surface cluster (merge train 221) grew
`native_table/net_events.rs` from 53 to 58 typed rows. Behind that stale
count sat the real problem: those five rows carry four runtime symbols that
had no entry in `scripts/native_result_ledger.tsv`, so the codegen table
declared a result class the provider inventory had no opinion about — and an
unclassified `result_kind` misrepresents to the GC what a native call
returns. Because `check()` raises on its first failure, the row-count check
never let the classification-coverage check run, so bumping the constant
alone would have turned the gate green and shipped the real defect.

Each provider was read rather than name-matched. All four return their
`handle: i64` argument unchanged — a `next_id_or_throw()` registry id and key
into `statics::sockets()`, not a heap address — so all four are
`NR_HANDLE_ID` (`NativeRetKind::HandleId`, "an integer registry id or
provider sentinel"):

- `js_ext_net_socket_on` — `crates/perry-ext-net/src/handle_exports.rs`
- `js_net_socket_prepend_listener` — `crates/perry-ext-net/src/lifecycle.rs`
- `js_net_socket_prepend_once_listener` — `crates/perry-ext-net/src/lifecycle.rs`
- `js_net_socket_unpipe` — `crates/perry-ext-net/src/pipe.rs`

Four symbols across five rows: `js_ext_net_socket_on` backs both the `on` and
the `addListener` rows. The sibling `js_net_socket_pipe` returns `f64` under
`ret: NR_F64`, which the scanner does not classify, so it needs no row.

`EXPECTED_ROWS` 371 → 376 and `EXPECTED_PROVIDERS` 322 → 326, with the
existing explanatory comment extended to attribute the delta to #10658 and
train 221. The gate was re-proved to bite: deleting a new row reddens it on
the provider count, deleting it with the count adjusted reddens it naming the
symbol, and misclassifying `js_net_socket_unpipe` as `NR_GCPTR` reddens it on
the table/provider disagreement.
