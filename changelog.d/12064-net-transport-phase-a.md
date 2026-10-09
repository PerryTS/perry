`net.Socket` and `net.Server` are native payloads with a TransportCore, and
their completions route by link token through the payload cell. perry-ext-net
no longer keeps id-keyed socket or server tables (19 tables and 10 producers
in the native handle ledger, now 0 and 0), so a Socket that closes and is
dropped is collected and its WeakRefs clear, as in Node.

HTTP/1 server and client connections are `net.Socket` payloads. An HTTP
`upgrade` is a single store to the connection's route: the listener gets the
same Socket, with the bytes that arrived with the head as `head` and every
later byte as its own `data`. The upgrade listener's socket parameter is an
ordinary value; the compiler no longer tags it as a `ws` Client, which used to
register its `data` listener in a ws id table that never fired. A server with
an attached `WebSocketServer` still hands the listener a ws client handle,
which dispatches through ws's own handle methods.

The socket-open audit now requires one `SocketFields.opened` declaration and
live connect/accept transitions, and rejects duplicate Rust/JS flags and
id-keyed maps. Bun's duplicate `bunOpened` latch is removed: its existing
pending connect promise selects `connectError`. The workflow step is retained.

Registry lifetime census: current main has 249 maps (167 with removal paths),
phase A has 225 (146 with removal paths). All 24 disappearing identities are
net/HTTP/TLS tables; none are added. Parser-liveness floors are adjusted from
250/170 to 220/140 solely because those tables were deleted.

Review notes: main already violates the native handle ledger because
`buffer/header.rs` moved to `buffer/store/layout.rs`. The seven ws Client
codegen rows and their manifest entries are unused after removal of the HTTP
upgrade parameter hint; delete them in the immediate follow-up. Linux ext-ws
lib tests fail to compile on main (`js_shadow_frame_push` is absent with native
stack maps). Main also fails issue_9619's manual/callback-only path and
`nested_object_literal_ws_inbound`: ws handleUpgrade has no raw-socket path.

Validation uses main a9834307e829 (the newest origin/main at the final rebase)
and separate qb6 targets. Script lint: main 102/125 pass, lane 103/125 pass;
the remaining failures and strict Clippy failures are inherited, with no new
diagnostics. Lean dependencies and pruned feature builds pass. The full lane
gap subset, with its echo fixture running, is 88 pass / 15 mismatch / 0 timeout
against the supplied main's 85 / 16 / 1: zero regressions. The worker completes
25 rounds, ws_client passes, and the new upgrade fixture passes.

This is a review bundle, with performance acceptance still blocked. All eight
programs match Node for five interleaved instructions/RSS runs on CPUs 0-55
with ASLR disabled. HTTP 10k adds 24.606% instructions and net echo 10k adds
36.939%; repeated payload validation/root access and event argument allocation
outweigh the deleted table lookups. Net's extra minor collection promotes an
old arena; its RSS increase survives THP-off. Fix-forward: reuse validation
within a callback-free operation, use existing rooted emitter arguments, and
avoid empty write-ack callback/provider work while preserving async_hooks.
The small tsc increase (+0.029%) is not fully attributed. P0's global
bound-method constructor guard also compares module/export names; replace
that special case with captured canonical constructor metadata to meet the
architecture policy. No performance acceptance is claimed for this bundle.
