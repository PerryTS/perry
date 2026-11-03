`--target windows-winui` now renders Perry's core UI controls with native WinUI 3 / Fluent controls and Mica chrome through Windows Reactor. The compiler deploys the required unpackaged Windows App SDK bootstrap assets automatically, while the default `--target windows` backend remains unchanged.

The vendored upstream `windows-rs` / Windows Reactor snapshot now lives in
`third_party/windows-winui/` (outside `crates/`) and is listed in
`[workspace] exclude`, so it is not a workspace member: `cargo test
--workspace` no longer compiles it and Perry's `[workspace.lints]` policy is
no longer applied to third-party code. It could not stay under
`crates/perry-ui-windows-winui/vendor/` — Cargo's `exclude` loses to an
explicit `members` entry that is a path prefix of the excluded directory, so
while it sat there the 13 vendored crates were implicit workspace members and
no `exclude` entry could remove them. `perry-ui-windows-winui` still resolves
them through a normal `path` dependency edge, and the only `Cargo.lock` change
is the loss of three dev-dependency edges that belong to the vendored crates.

The WinUI backend now registers a GC root scanner
(`perry-ui-windows-winui/src/gc.rs`, following #8713). Every persistent
JavaScript callback it stores is a raw closure pointer unboxed by
`js_nanbox_get_pointer`, so `widgets::NODES` (the generic `on_click` plus the
`Button`/`TextField`/`SecureField`/`Toggle`/`Slider` handlers) and
`app::{ON_ACTIVATE, ON_TERMINATE, PENDING_TIMERS}` are GC roots that an
evacuating collection has to rewrite. Registration also chains
`perry-ui-windows`' scanner, which the Fluent path previously never armed
because it shadows `app_create`, and reaches this crate's own `#[path]`-included
copy of `state.rs`. The per-kind match is exhaustive with no `_` arm so a new
callback-bearing `NodeKind` cannot silently drop its root.
