// Adapted from Wasmtime 48.0.5; Apache-2.0 WITH LLVM-exception. See PROVENANCE.md.
use super::open_parent;
use crate::filesystem::primitives::{set_times_nofollow_unchecked, MaybeOwnedFile};
use std::path::Path;
use std::time::SystemTime;
use std::{fs, io};

#[inline]
pub(crate) fn set_times_nofollow(
    start: &fs::File,
    path: &Path,
    atime: Option<SystemTime>,
    mtime: Option<SystemTime>,
) -> io::Result<()> {
    let start = MaybeOwnedFile::borrowed(start);

    let (dir, basename) = open_parent(start, path)?;

    set_times_nofollow_unchecked(&dir, basename.as_ref(), atime, mtime)
}
