### Fixed

`net.Socket.read()` now implements the readable-stream pull contract for both
bundled and optimized net providers. Pull-mode consumers such as undici receive
queued bytes as `Buffer` values and `null` when the socket queue is empty,
instead of falling through to `undefined`.

### Internal

`crates/perry-stdlib/src/net/mod.rs` crossed the 2000-line file cap with this
change, so it was split into sibling modules — `value_helpers.rs` (NaN-boxed
JS value/object readers), `tls_config.rs` (TLS option parsing + rustls
connector construction) and `socket_task.rs` (the per-socket tokio task) —
leaving `mod.rs` at 1181 lines. Pure move: no renames, no behaviour change,
and all 18 `#[no_mangle]` exports stay in `mod.rs`. `net/tls_verifier.rs`
swapped its `use super::*;` for explicit `rustls` imports, since the glob no
longer picked those up through the parent.

The new `net::read` dispatch row also needed its `API_MANIFEST` counterpart
(`perry-codegen`'s `every_dispatch_entry_has_manifest_counterpart` asserts the
two tables agree), plus the regenerated `docs/src/api/reference.md` line.
