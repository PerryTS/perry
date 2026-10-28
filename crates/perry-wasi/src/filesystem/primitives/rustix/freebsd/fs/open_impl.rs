// Adapted from Wasmtime 48.0.5; Apache-2.0 WITH LLVM-exception. See PROVENANCE.md.
use super::super::super::fs::compute_oflags;
use crate::filesystem::primitives::{errors, manually, OpenOptions};
use rustix::fs::{openat, Mode, OFlags, RawMode};
use std::path::Path;
use std::{fs, io};

pub(crate) fn open_impl(
    start: &fs::File,
    path: &Path,
    options: &OpenOptions,
) -> io::Result<fs::File> {
    if !super::beneath_supported() {
        return manually::open(start, path, options);
    }

    let oflags = compute_oflags(options)? | OFlags::RESOLVE_BENEATH;

    let mode = if oflags.contains(OFlags::CREATE) {
        Mode::from_bits((options.ext.mode & 0o7777) as RawMode).unwrap()
    } else {
        Mode::empty()
    };

    match openat(start, path, oflags, mode) {
        Ok(file) => Ok(file.into()),
        Err(rustix::io::Errno::NOTCAPABLE) => Err(errors::escape_attempt()),
        Err(err) => Err(err.into()),
    }
}
