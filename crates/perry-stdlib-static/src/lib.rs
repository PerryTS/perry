//! Staticlib wrapper for `perry-stdlib` (#5422). See `perry-runtime-static`'s
//! lib.rs for the rationale. `[lib] name = "perry_stdlib"` keeps the canonical
//! `libperry_stdlib.a` / `perry_stdlib.lib` output basename, and depending on
//! `perry-stdlib` links its full object code (including the runtime symbols it
//! re-bundles via its own `perry-runtime` / `perry-updater` deps) into the
//! archive.
extern crate perry_stdlib;
