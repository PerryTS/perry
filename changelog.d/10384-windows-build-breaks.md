### Fixed

- **Windows: `perry.exe` failed to link with 7 × LNK2019 on the `js_lru_cache_*`
  ABI.** `perry-runtime/src/lru_subclass.rs` (the `class X extends LRUCache`
  support added with the LRUCache subclass-init) declares `js_lru_cache_new`,
  `_get`, `_set`, `_has`, `_delete`, `_clear` and `_peek` as `extern "C"`, and
  leaves them to whichever cache provider the *program* links —
  `perry-ext-lru-cache` or perry-stdlib's `bundled-lru-cache`. A Rust binary
  that links perry-runtime without either still carries the references, and CI
  builds two such binaries on every Windows leg: the `perry` compiler
  (`cargo build -p perry …`) and the crate's own `--lib` test harness.

  That is invisible everywhere else because those linkers dead-strip *before*
  they report. `ld64 -dead_strip` drops the thunks out of a binary that never
  calls them and the references go with them — verified locally: the same
  `cargo build --profile perry-dev -p perry -p perry-runtime-static -p
  perry-stdlib-static` that fails on `windows-arm64-build` succeeds on macOS,
  and the linked `target/perry-dev/perry` carries no `js_lru_cache_subclass_init`
  symbol at all while the rlib it links still shows all seven as `U`.
  `link.exe` resolves symbols before `/OPT:REF`, so on MSVC the same inputs are
  seven hard unresolved externals.

  A Cargo feature cannot express "this link has no provider": the Windows job
  builds `-p perry -p perry-runtime-static -p perry-stdlib-static` in **one**
  invocation, so perry-stdlib's `perry-runtime/stdlib` feature is unified onto
  the copy of perry-runtime that `perry` links even though perry-stdlib is not
  in that binary's link (checked against `cargo build --unit-graph`: the single
  `perry-runtime` rlib unit has `stdlib` enabled). Anything gated on `stdlib` —
  `crate::stdlib_stubs`, an `external-*-symbols` flag — is compiled out in
  exactly the configuration that fails.

  Fixed with MSVC's spelling of a weak default: an `#[cfg(all(windows,
  target_env = "msvc"))]` module in `lru_subclass.rs` emits one
  `/ALTERNATENAME:js_lru_cache_<op>=perry_lru_cache_absent_<op>` linker
  directive per symbol via `.drectve`, alongside no-op fallbacks that report
  through `stub_diag::perry_stub_warn`. `link.exe` substitutes an alternate
  only for a symbol still undefined after every input has been read, so a
  program that does link `perry_stdlib.lib` or the ext archive binds the real
  implementation and never reaches these — unlike an unconditional definition,
  which would either duplicate (LNK2005) or silently shadow the real cache.
  The fallbacks share a codegen unit with the thunks whose references they
  answer. `js_lru_cache_new` answering `0` is already the module's "no cache"
  path: subclass-init returns `this` with no method installed, so a `.get()`
  throws `is not a function` at the call site, the same failure the module
  already chooses for `forEach`/`dispose`/`fetch`.

- **Windows: `perry-ui-windows-winui` failed to compile —
  ``cannot find function `reorder_child` in module `widgets` ``.**
  `perry-ui-windows-winui` `#[path]`-includes perry-ui-windows'
  `src/ffi/mod.rs`, so `ffi/widget_layout_extras.rs`'s
  `perry_ui_widget_reorder_child` resolves `widgets::` against winui's **own**
  `src/widgets.rs` — which had `add_child_at`, `remove_child` and
  `clear_children` but no `reorder_child`. Added it in the shape every other
  entry in that module uses: delegate to `perry_ui_windows::widgets` when the
  Fluent backend is inactive, otherwise reorder the node's own child list under
  `with_node_mut`, with the same out-of-range / no-op guards as the Win32
  implementation. `perry_ui_widget_reorder_child` is a live entry in the
  UI dispatch table and every other backend (macOS, GTK4, iOS, tvOS, visionOS,
  Android, watchOS, Win32) implements it, so cfg'ing the caller out on Windows
  would have been a regression, not a fix.
