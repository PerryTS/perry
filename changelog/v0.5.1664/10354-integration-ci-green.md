### Fixed

- **turnloop HTTP/2 listener lost `server.noDelay`, and did not compile.** The
  P5 listen fix gave `perry_ffi::turnloop_net::tcp_listen` a seventh parameter
  (`nodelay`), splitting it out of `reuse_port`, which it had been silently
  landing in. The HTTP/2 lane wrote `turnloop_h2::listen` against the six-argument
  signature, so the merge of the two lanes produced a call site that no longer
  compiled (`E0061: this function takes 7 arguments but 6 arguments were
  supplied`). This is what reddened `ext-link`, `CI / check` (Clippy) and one
  arm of `CI / warnings` — one semantic merge conflict reported as three
  unrelated gate failures.

  Fixed by plumbing the option rather than passing a constant:
  `try_listen_on_turnloop` reads `server.base.no_delay` under the same handle
  borrow it already takes for the TLS config and the settings, and hands it to
  `turnloop_h2::listen`. The turnloop HTTP/2 path now honours `noDelay` the way
  the hyper HTTP/2 path already did via `apply_accept_no_delay`, and the way
  Node defaults it (true).

- **`turnloop_proc`'s datagram surface failed `cargo check -p perry --bins`.**
  `dgram_reactor` is `#[cfg(feature = "mod-dgram")]` and `turnloop_proc` is not,
  and `perry` depends on `perry-runtime` with `default-features = false` — so in
  the binary's feature set the datagram half of the P2 handle table has no
  consumer and nine items read as dead code, which `RUSTFLAGS=-D warnings` turns
  into nine errors. Marked `#[cfg_attr(not(feature = "mod-dgram"), allow(dead_code))]`
  (the shape `node_submodules` already uses for `mod-node-test`), so the
  dead-code gate stays **live** in the configuration that has the consumer
  instead of being blanket-silenced.

- **`check_thread_locals.py` counted test-only declarations as shipping code.**
  `CFG_TEST_MOD_RE` required `#[cfg(test)]` to be *adjacent* to its `mod`, but
  `#[cfg(test)] #[path = "tests.rs"] mod tests;` is the spelling 58 declarations
  in `perry-runtime` use. Every one of those files read as shipping code, so a
  `thread_local!` in one was counted against a build it cannot appear in — five
  of the eleven reported violations were this false positive. The checker now
  tolerates intervening attributes (they only ever *narrow* the cfg), and a new
  self-test case asserts both directions for the separated spelling; reverting
  the regex makes that case fail, so the fix is covered rather than merely
  applied. The six genuinely-shipping declarations left over — four in
  `event_pump::agent_loop`, one each in `turnloop_net` and `turnloop_proc`, all
  on the event loop's hot path — were converted to `crate::perry_thread_local!`
  rather than recorded as cold.

- **The #8075 provider-dylib GC gate broke on a freshly published dependency.**
  `tests/fixtures/issue_8075_provider_gc/stdlib-provider` carries `[workspace]`,
  so it resolves independently of the repository lock — and an independent
  resolution is subject to `.cargo/config.toml`'s `min-publish-age` soak, which
  the pinned nightly makes live. `turnloop-http 0.1.0-alpha.5`, seven hours old
  against a seven-day window, therefore failed *only* there, reported as a GC
  gate failure. `scripts/gc_provider_dylib_gate.sh` now seeds the fixture with
  the repository's own `Cargo.lock` before building it, so the fixture adopts
  nothing new and the soak keeps applying to the workspace lock, where it
  belongs.
