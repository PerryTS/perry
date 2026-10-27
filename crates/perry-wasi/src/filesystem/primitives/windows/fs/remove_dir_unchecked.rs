// Adapted from Wasmtime 48.0.5; Apache-2.0 WITH LLVM-exception. See PROVENANCE.md.
use super::get_path::concatenate;
use std::path::Path;
use std::{fs, io};

/// *Unsandboxed* function similar to `remove_dir`, but which does not perform
/// sandboxing.
pub(crate) fn remove_dir_unchecked(start: &fs::File, path: &Path) -> io::Result<()> {
    let full_path = concatenate(start, path)?;
    fs::remove_dir(full_path)
}
