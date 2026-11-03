//! `PERRY_SOURCE_READ_LOG=<path>`: append one line per read of retained
//! function or class source (`kind`, byte length, FNV-1a of the bytes). It is
//! the runtime witness for the compiler's `PERRY_SOURCE_NEED_REPORT` census:
//! every source a run reads must belong to an owner the census keeps. Off, a
//! read costs one initialized `OnceLock` load; source reads are cold paths.
use std::io::Write;
use std::sync::{Mutex, OnceLock};

static LOG: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();

#[cold]
fn open() -> Option<Mutex<std::fs::File>> {
    let path = std::env::var_os("PERRY_SOURCE_READ_LOG")?;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .ok()
        .map(Mutex::new)
}

#[inline]
pub(crate) fn note(kind: &str, bytes: &[u8]) {
    if let Some(file) = LOG.get_or_init(open) {
        let mut h: u64 = 0xcbf29ce484222325;
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        if let Ok(mut f) = file.lock() {
            let _ = writeln!(f, "{kind}\t{}\t{h:016x}", bytes.len());
        }
    }
}
