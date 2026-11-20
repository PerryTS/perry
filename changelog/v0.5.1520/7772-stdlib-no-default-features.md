### Fixed

- **`perry-stdlib` builds with `--no-default-features` again (#7764).** That is the configuration the auto-optimize relink uses, so while it was broken every `perry` compile that triggered auto-optimize silently fell back to the prebuilt archives, and ad-hoc builds needed `PERRY_NO_AUTO_OPTIMIZE=1` as a workaround.

  Twelve errors, from two causes, both violations of the contract `common/mod.rs` states in prose: *"Always-on code that references it must also be `#[cfg(feature = "async-runtime")]`-gated."*

  **One** was #7745's omission, exactly as the issue diagnosed: the `js_set_native_events_dispatch` registration referenced `crate::events` without the `#[cfg(feature = "bundled-events")]` that gates the module. The neighbouring registrations in the same function are gated (`database-sqlite` on the next line), which is what makes it an omission rather than a decision.

  **Eleven** were `worker_threads` — always-on, and referencing `common::async_bridge` across five files. Neither obvious repair works: two sites are value-producing (`js_promise_new_for_native_resolution`), so `#[cfg]` on the statement leaves nothing to return; and gating the whole `worker_threads` module is worse, because it has no feature of its own, so its FFI symbols would vanish from the stripped archive and a program importing `node:worker_threads` would fail to LINK — trading a build error for the #7629 family of failure.

  So `worker_threads/async_shim.rs` provides the four entry points in both configurations: forwarding to `async_bridge` when it is compiled in, and settling **inline** when it is not. That is not invented semantics — the queue exists to hand work to the pump, and with no pump there is nothing to hand it to, so doing the same work synchronously reaches the same observable end state. The pinning `js_promise_new_for_native_resolution` performs is likewise a consequence of deferral, and an inline settle spans no collection point, so a plain `js_promise_new` is its correct counterpart.

  Verified in BOTH directions — `cargo build -p perry-stdlib` and `--no-default-features` each build clean — because the first cut of the shim accidentally imported itself, which only the default-features build could see.
