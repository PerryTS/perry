Fix `socket.connecting` / `pending` / `readyState` never clearing on the
turnloop connect path.

`turnloop_io.rs`'s `on_connect` — the turnloop replacement for the tokio
connect paths — set `is_open`, `local_addr` and `remote_addr`, but not
`has_opened` or `connecting`. The tokio paths it replaces (`lib.rs:1312`,
`lib.rs:1494`) write all five together, and `ipc.rs:236` does too.

Three JS-visible symptoms all followed from those two missing writes:

- `socket.connecting` stayed `true` for the socket's entire connected life.
- `socket.readyState` returned `"opening"` throughout, because
  `lifecycle.rs:275` returns `"opening"` whenever `connecting` is set.
- `socket.pending` stayed `true`, because the getter keys on `has_opened`
  (`lifecycle.rs:144`) to distinguish "never opened" from "opened and since
  closed".

Everything else was already correct, which is why this was narrow rather than a
broken lifecycle: the `'connect'` event fired, `writable` / `readable` /
`destroyed` tracked properly (they read none of those two fields), and the
`'close'` line was right because the close path clears `connecting` itself.

Caught by `test_gap_net_socket_surface_cluster`, which covers four
package-audit issues (#10441/#10442/#10444/#10465) found compiling mysql2, pg,
redis and ws natively. This is driver-facing: a driver that waits for
`readyState === "open"`, or guards on `!socket.connecting` before writing,
would never proceed.

The writes are placed at the `push_event(Connect)` tick rather than in the
`is_open` block above it, so that the `begin_client_upgrade` failure path —
which returns early to destroy the socket — cannot record it as opened.

For a direct-TLS socket this clears the flags at TCP-connect rather than at
handshake completion. That is deliberate and matches Node: a TLS socket's
underlying connection completes at the TCP level, which is when `'connect'`
fires and `connecting` goes false, with the handshake signalled separately by
`'secureConnect'`. The tokio path at `lib.rs:1494`, which holds `connecting`
true until the transport including TLS is established, is the deviation. The
reasoning is in the code comment too, because the placement reads as premature
and would otherwise invite being "fixed" back. Nothing pins it yet — the parity
fixture is plain-socket only, so both timings pass today (#11056).

Note `lib.rs:296` documents the invariant this broke: `has_opened` is "set
alongside every `is_open = true` transition". Nothing enforces it. The five
`is_open = true` sites were audited — the only other one that omits the
siblings is `tls.rs:70`, which is correct to: it is a synthetic socket for an
already-aborted connect that never opens.
