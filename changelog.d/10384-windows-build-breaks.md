Unbreak both Windows CI legs on `main`.

`windows-arm64-build` failed with `LNK1120: 7 unresolved externals` — the whole
`js_lru_cache_*` ABI — when linking **the `perry` compiler itself**.
`perry-runtime/src/lru_subclass.rs` declares that ABI `extern "C"` and leaves it
to whichever cache provider the program links; `perry` links neither provider, so
its link carries seven undefined references. Every other target hides this
because its linker dead-strips before it reports (the same command succeeds on
macOS while the rlib still shows all seven as `U`), whereas `link.exe` resolves
symbols before `/OPT:REF`.

A Cargo feature cannot express "this link has no provider": the Windows job
builds `-p perry -p perry-runtime-static -p perry-stdlib-static` in one
invocation, so perry-stdlib unifies `perry-runtime/stdlib` onto the copy of
perry-runtime that `perry` links, and anything gated on `stdlib` — including the
existing `stdlib_stubs` mechanism — is compiled out in exactly the failing
configuration. Fixed with `/ALTERNATENAME` directives in `.drectve` behind
`cfg(all(windows, target_env = "msvc"))` plus no-op fallbacks reporting through
`perry_stub_warn`; `link.exe` substitutes an alternate only for a symbol still
undefined after all inputs are read, so a real provider always wins.

`windows-build` failed with `error[E0425]: cannot find function reorder_child in
module widgets`, in `perry-ui-windows-winui`: it `#[path]`-includes
perry-ui-windows' `ffi/mod.rs`, so `widgets::` resolves against winui's own
module, which never gained `reorder_child`. Added in the module's established
shape — delegate to the Win32 implementation when Fluent is inactive, otherwise
reorder the node's children under `with_node_mut`.

Neither Windows job runs in the PR tier (`ci_plan.py`: sweep and full only), so
the fix was validated by emitting the COFF object for
`x86_64-pc-windows-msvc` and confirming the `.drectve` contents and symbol
classes directly; no Windows link was performed.
