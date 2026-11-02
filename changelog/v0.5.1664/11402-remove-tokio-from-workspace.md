- **tokio is removed from the workspace** (final tokio lane). No crate depends on
  tokio any more — normal, dev or build — and `Cargo.lock` holds no `tokio`,
  `tokio-macros` or `mio` 1.x; turnloop is the only event loop, in every crate
  and every test.
  - `perry-stdlib`: deleted `common/tokio_bridge.rs`, the `async-runtime` feature
    (also dropped from `full`) and `dep:tokio`. `common::async_bridge`
    (`async-bridge`) is the only bridge. `perry_ffi_spawn_blocking` keeps only its
    turnloop arm (long-occupancy worker set, OS-thread fallback);
    `perry_ffi_run_pending` is one bounded loop turn. The bundled `sharp` module's
    `toFile` / `toBuffer` / `metadata` now run on turnloop's pool and build their
    result strings on the main thread (they used to build them on a tokio worker,
    the #1824 arena hazard); `bundled-sharp` now implies `async-bridge` +
    `perry-base64` so it compiles on its own.
  - `perry-ffi`: **retired `spawn_async` and `spawn_blocking_with_reactor`** and
    their C symbols (`perry_ffi_spawn_async`,
    `perry_ffi_spawn_blocking_with_reactor`). Their contract ("tokio's reactor is
    ambient") cannot be met without tokio, and no in-tree or known out-of-tree
    wrapper called them. This is an ABI break inside `0.5.x`, documented under
    "Retired" in `docs/src/native-libraries/abi.md` with the migration table.
  - CLI: deleted the #7629 shared-tokio link check (`compile/shared_tokio.rs`),
    `binding_bundles_tokio`, the driver's `async-runtime` selection and the
    no-auto tokio warning. The #507 co-build set is kept, renamed
    `binding_cobuilds_with_stdlib`; its cache-key / build-stamp segment is now
    `cobuild=` (one-time rebuild of `target/perry-auto-*` dirs). New test
    `no_feature_selection_reaches_tokio` asserts no import selects a tokio
    feature and that perry-stdlib's manifest declares neither `async-runtime` nor
    a `tokio` dependency.
  - Workspace: `tokio`, `tokio-tungstenite`, `tokio-rustls`, `reqwest`, `hyper`
    and `hyper-util` removed from `[workspace.dependencies]` (all unused).
    `deny.toml` bans `tokio`, `tokio-rustls`, `tokio-tungstenite`, `tokio-util`.
  - `scripts/tokio_inventory.py` is now a ban, not a ratchet: any tokio manifest
    edge (any kind, any target), any tokio package in `Cargo.lock` or any
    non-comment `tokio::` in `crates/*` fails `lint`, with no allowlist.
    `tungstenite` (synchronous; perry-ui-android, turnloop-websocket's codec) and
    `lettre` (message builder only) are recorded as `not_tokio`, and the gate
    verifies from `Cargo.lock` that neither depends on tokio. Self-test plants 8
    violations.
  - Docs: `docs/turnloop/p8-report.md` gains an epilogue; the ABI reference,
    authoring guide, overview, HTTP page and the two "backed by Tokio" sentences
    in the language docs are corrected.
