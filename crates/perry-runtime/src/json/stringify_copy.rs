//! Bounded copies for the short keys, numbers and strings in scalar plans.
//! Every load and store stays inside the requested byte range. Long string
//! payloads keep the platform bulk copy.

/// Copy `len` bytes between nonoverlapping valid regions, with `len <= 32`.
/// The end stores may overlap the beginning stores; they write identical
/// bytes to that overlap. No padding may be read or overwritten.
#[inline(always)]
pub(super) unsafe fn copy_short(source: *const u8, output: *mut u8, len: usize) {
    if len >= 16 {
        let first = source.cast::<u128>().read_unaligned();
        let last = source.add(len - 16).cast::<u128>().read_unaligned();
        // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
        output.cast::<u128>().write_unaligned(first);
        // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
        output.add(len - 16).cast::<u128>().write_unaligned(last);
    } else if len >= 8 {
        let first = source.cast::<u64>().read_unaligned();
        let last = source.add(len - 8).cast::<u64>().read_unaligned();
        // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
        output.cast::<u64>().write_unaligned(first);
        // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
        output.add(len - 8).cast::<u64>().write_unaligned(last);
    } else if len >= 4 {
        let first = source.cast::<u32>().read_unaligned();
        let last = source.add(len - 4).cast::<u32>().read_unaligned();
        // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
        output.cast::<u32>().write_unaligned(first);
        // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
        output.add(len - 4).cast::<u32>().write_unaligned(last);
    } else if len >= 2 {
        let first = source.cast::<u16>().read_unaligned();
        let last = source.add(len - 2).cast::<u16>().read_unaligned();
        // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
        output.cast::<u16>().write_unaligned(first);
        // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
        output.add(len - 2).cast::<u16>().write_unaligned(last);
    } else if len == 1 {
        // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
        output.write(source.read());
    }
}

/// Same validity and nonoverlap requirements as `copy_nonoverlapping`.
#[inline(always)]
pub(super) unsafe fn copy_bytes(source: *const u8, output: *mut u8, len: usize) {
    if len <= 32 {
        copy_short(source, output, len);
    } else {
        std::ptr::copy_nonoverlapping(source, output, len);
    }
}

#[cfg(test)]
#[path = "stringify_copy_tests.rs"]
mod tests;
