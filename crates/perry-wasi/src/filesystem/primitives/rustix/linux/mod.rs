// Adapted from Wasmtime 48.0.5; Apache-2.0 WITH LLVM-exception. See PROVENANCE.md.
//! Following [`std`], we don't carry workarounds for Linux versions
//! older than 2.6.32.
//!
//! [`std`]: https://github.com/rust-lang/rust/pull/74163

pub(crate) mod fs;
