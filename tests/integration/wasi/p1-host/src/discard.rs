//! `perry_host.discard_pages` for `PERRY_WASM_DISCARD=1` modules
//! (docs/WASI_P1_OUTPUT.md).
//!
//! `discard_pages(ptr, len)` asks the host to drop the physical pages behind
//! `len` bytes of linear memory at `ptr`. The guest passes 4096-multiples. A
//! return of 1 means the whole range now reads as zero; 0 means the host did
//! nothing, which the guest takes as "never ask again". A range outside the
//! memory traps. This host answers 0 for a misaligned or empty range, and for
//! everything when started with `--no-discard`.
//!
//! The pages are dropped with `madvise(MADV_DONTNEED)` on Linux, which hands
//! back zero pages for private anonymous memory. A copy-on-write page of the
//! module's data image could come back with its initial contents, so the range
//! is read back and any byte that is not zero is cleared. Elsewhere the range
//! is zero-filled, which reclaims nothing but keeps the contract.

use crate::preview1::Host;
use wasmtime::{Caller, Extern};

/// The size the contract works in, whatever the host's own page size is.
const PAGE: u32 = 4096;

pub fn discard_pages(c: &mut Caller<'_, Host>, ptr: u32, len: u32) -> wasmtime::Result<u32> {
    c.data_mut().stats.discard_calls += 1;
    let Some(Extern::Memory(memory)) = c.get_export("memory") else {
        return Err(wasmtime::Error::msg(
            "discard_pages: the guest exports no memory",
        ));
    };
    let size = memory.data_size(&*c);
    let end = (ptr as usize)
        .checked_add(len as usize)
        .filter(|end| *end <= size)
        .ok_or_else(|| {
            wasmtime::Error::msg(format!(
                "discard_pages: [{ptr:#x}, +{len:#x}) is outside the {size:#x}-byte memory"
            ))
        })?;
    if !c.data().discard_answers || len == 0 || ptr % PAGE != 0 || len % PAGE != 0 {
        return Ok(0);
    }
    let range = &mut memory.data_mut(&mut *c)[ptr as usize..end];
    zero(range);
    c.data_mut().stats.discard_bytes += u64::from(len);
    Ok(1)
}

/// Make `range` read as zero, giving its pages back to the OS where the host
/// can.
fn zero(range: &mut [u8]) {
    #[cfg(target_os = "linux")]
    {
        // madvise wants the host's page size, which can exceed 4096; the edges
        // that do not fill a host page are cleared below.
        let host_page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;
        let start = range.as_ptr() as usize;
        let from = start.next_multiple_of(host_page);
        let to = (start + range.len()) / host_page * host_page;
        if from < to {
            unsafe { libc::madvise(from as *mut libc::c_void, to - from, libc::MADV_DONTNEED) };
        }
    }
    // Reads back the range (a dropped anonymous page reads as the shared zero
    // page) and clears whatever did not come back zero.
    for chunk in range.chunks_mut(PAGE as usize) {
        if chunk.iter().any(|b| *b != 0) {
            chunk.fill(0);
        }
    }
}
