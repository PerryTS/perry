// Adapted from Wasmtime 48.0.5; Apache-2.0 WITH LLVM-exception. See PROVENANCE.md.
//! This defines `open`, the primary entrypoint to sandboxed file and directory
//! opening.

use crate::filesystem::primitives::{open_impl, OpenOptions};
use std::path::Path;
use std::{fs, io};

/// Perform an `openat`-like operation, ensuring that the resolution of the
/// path never escapes the directory tree rooted at `start`.
#[inline]
pub fn open(start: &fs::File, path: &Path, options: &OpenOptions) -> io::Result<fs::File> {
    // Call the underlying implementation.
    open_impl(start, path, options)
}
