### Changed

- Bumped the turnloop family from `0.1.0-alpha.6` to `0.1.0-alpha.8` in
  lockstep (`turnloop`, `turnloop-http`, `turnloop-tls`, `turnloop-websocket`,
  `turnloop-smtp`, `turnloop-redis`, `turnloop-mongodb`, plus the transitive
  `turnloop-wasi-random` and `turnloop-zstd-decoder`). These crates share one
  workspace version and require each other at `^0.1.0-alpha.N`, so they can
  only move together. Adopted under an owner-approved publish-age override;
  `cargo`'s `min-publish-age` has no per-package exclusion list, so the
  override is the recorded `CARGO_RESOLVER_INCOMPATIBLE_PUBLISH_AGE=allow`
  re-resolve, and `Cargo.lock` pins the exact artifacts. Every direct crate's
  checksum was verified against the crates.io API rather than against the
  publishing workflow going green.

alpha.7 → alpha.8 is purely additive: it adds `Detached::from_listener_fd`
(turnloop#108, Unix only) and semver-checks rated it API-compatible. Every
change described below is from alpha.6 → alpha.7, taken in the same commit
because the crates move in lockstep and Perry pins with `=`.

Three of the changes needed more than a mechanical edit.

**`turnloop-tls`'s `ring` became an optional-but-default feature, and Perry
asks for `default-features = false`.** In alpha.6 `ring` was an
*unconditional* dependency, so that setting — which is there only to keep the
`turnloop` feature (and its `turnloop-io` `LocalExecutor`) out — still left
Perry with ring. In alpha.7 it would have silently removed it, taking two
things with it. `turnloop_tls::tls_server_end_point` is `#[cfg(feature =
"ring")]`, and it is what `perry-tls-session` hands `turnloop-postgres` as RFC
5929 `tls-server-end-point` channel binding for SCRAM-SHA-256-PLUS, where a
missing or wrong digest makes the driver offer PLUS and then fail the server
signature. The alpha.7 `tls_server_end_point_hash` is **not** a drop-in
replacement: it returns *which* hash RFC 5929 §4.1 selects (`EndPointHash`),
not the computed `Digest`, so a caller must hash the leaf itself. Separately,
without `ring` the crate's `default_provider()` stops returning ring and
instead demands a rustls process-default provider, failing at **runtime** with
"no rustls crypto provider" rather than at compile time. The workspace pin now
names `features = ["ring"]` explicitly, which restores exactly what alpha.6
gave Perry and opts into nothing new. (`turnloop-http`'s new `ring` default
only forwards to `turnloop-tls` and has no ring-gated code of its own.)

**The http1 request-mode decoder now ends an upgrade with `Event::Upgrade`
instead of `Event::End`.** `turnloop_serve`'s connection loop already carried
an `Event::Upgrade` arm, written as unreachable ("kept so a later decoder that
does raise it cannot fall through") that routed straight to `Step::Upgrade`.
alpha.7 is that later decoder, so the arm went live — and because it already
existed, nothing about this was a compile error. Left alone it would have
bypassed both the attached-`WebSocketServer` precedence and the
`has_upgrade_listener` test (#4973: an upgrade with no listener is served as an
ordinary request), and a `CONNECT` — which sets the decoder's
`upgrade_request` but is not an `Upgrade` header, so never sets
`Building::upgrade` — would have stopped being dispatched as a request at all.
`Upgrade` now shares the `End` arm, so one copy of the routing policy serves
both. The stale doc comment on `Building::upgrade` that asserted the decoder
never raises it was corrected.

**`LocalExecutor::turn` no longer dropping foreign completions does not reach
Perry.** That change is `turnloop-io`'s executor, which Perry deliberately does
not use — it owns its own `turnloop::Loop` and routes tokens through it, for
exactly the reason the change fixes. Perry drives
`Driver::turn(timeout, &mut Completions)`, whose body is identical between
alpha.6 and alpha.7 apart from the new internal `self.budget()` argument to
`Backend::poll`. There is no completion-retention behaviour change on any path
Perry takes, and no completion leak.

The mechanical edits: `TcpOpts` and `ClientOptions` struct literals gained the
new `connect_timeout` / `provider` fields (both left at the default, which is
alpha.6's behaviour — no connect timeout, and the ring provider); the http2
`Event::Settings(..)` / `Event::Goaway { .. }` patterns were updated and the
new `Event::SettingsAck` is carried and ignored, matching the silent step it
replaced; and `turnloop-smtp`'s new `Event::BodyReady` got an arm documenting
that it cannot arrive here, because this module only ever sends whole messages
and never starts a streamed one.

Noted and deliberately **not** acted on, since a dependency bump is the wrong
place to add behaviour: alpha.7's `Event::Settings` now carries the peer's
actual `SettingsFrame` and `Event::Goaway` its RFC 9113 §6.8 debug data, both
of which this tree currently works around or drops; `turnloop-mongodb`'s
`WriteResult::parse` is now strict about `writeErrors` / `writeConcernError`
(the lenient parse moved to `WriteResult::decode`), which is exactly the
`Error::from_response` check `perry-ext-mongodb` already ran immediately before
each call, so the calls are now redundant rather than load-bearing and the
comment saying otherwise was corrected; and `turnloop`'s new `Loop::rebuild`
may suit `event_pump/agent_loop.rs`'s profile upgrade, which currently drops
and recreates a loop.

`turnloop-postgres` and `turnloop-mysql` are not dependencies of this
workspace, so the alpha.7 changes to them (`Error::Transport`,
`Value::Unknown`, `consume_output`, `Event::Eof`, TIME-as-String) reach nothing
here.
