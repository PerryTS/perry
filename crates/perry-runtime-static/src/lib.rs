//! Staticlib wrapper for `perry-runtime` (#5422).
//!
//! `perry-runtime` is now an rlib-only crate so a plain `cargo build` of the
//! workspace no longer emits the heavy `libperry_runtime.a` as a side effect.
//! This thin wrapper is the only crate with `crate-type = ["staticlib"]`, so
//! the archive is produced exactly when requested (`cargo build -p
//! perry-runtime-static`). Its `[lib] name = "perry_runtime"` keeps the output
//! basename (`libperry_runtime.a` / `perry_runtime.lib`) that every consumer —
//! `library_search.rs`, the auto-optimize linker, `stage-npm.sh`, the symbol
//! guard — already resolves.
//!
//! Pulling `perry-runtime` in as a dependency links all of its object code
//! (including the `#[no_mangle]` / `#[used]` C API surface that generated
//! native code calls) into this archive.
extern crate perry_runtime;
