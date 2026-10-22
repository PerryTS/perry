// Adapted from Wasmtime 48.0.5; Apache-2.0 WITH LLVM-exception. See PROVENANCE.md.
//! This defines `stat`, the primary entrypoint to sandboxed metadata querying.

use crate::filesystem::primitives::{stat_impl, FollowSymlinks, Metadata};
use std::path::Path;
use std::{fs, io};

/// Perform an `fstatat`-like operation, ensuring that the resolution of the
/// path never escapes the directory tree rooted at `start`.
#[inline]
pub fn stat(start: &fs::File, path: &Path, follow: FollowSymlinks) -> io::Result<Metadata> {
    // Call the underlying implementation.
    stat_impl(start, path, follow)
}
