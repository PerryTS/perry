// Adapted from Wasmtime 48.0.5; Apache-2.0 WITH LLVM-exception. See PROVENANCE.md.
use std::io;

#[cfg(not(windows))]
pub(crate) use crate::filesystem::primitives::rustix::fs::errors::*;
#[cfg(windows)]
pub(crate) use crate::filesystem::primitives::windows::fs::errors::*;

#[cold]
pub(crate) fn escape_attempt() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "a path led outside of the filesystem",
    )
}
