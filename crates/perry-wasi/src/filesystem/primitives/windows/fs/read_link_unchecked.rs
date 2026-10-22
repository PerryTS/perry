// Adapted from Wasmtime 48.0.5; Apache-2.0 WITH LLVM-exception. See PROVENANCE.md.
use super::get_path::concatenate;
use std::path::{Path, PathBuf};
use std::{fs, io};

/// *Unsandboxed* function similar to `read_link`, but which does not perform
/// sandboxing.
pub(crate) fn read_link_unchecked(
    start: &fs::File,
    path: &Path,
    _reuse: PathBuf,
) -> io::Result<PathBuf> {
    let full_path = concatenate(start, path)?;
    fs::read_link(full_path)
}
