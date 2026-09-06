//! Proof for permanent array-growth aliases in evacuation diagnostics.
//!
//! Growth retains its old header for aliases. A copied minor need not rewrite
//! an old owner's old-to-old edge. Evacuation stubs have a different lifetime
//! and must still be rejected if a reference survives their rewrite phase.

/// Forwarded arrays have no ordinary payload layout. Use its otherwise-unused
/// state 0b11 to identify a growth stub explicitly; live array layouts use
/// unknown (00), pointer-free (01), or side-mask (10). Set only AFTER layout
/// transfer to the live target, so the marker is never copied onto that target.
pub(super) const GROWTH_STUB_LAYOUT: u16 = crate::gc::GC_LAYOUT_STATE_MASK;

/// A diagnostic exemption requires a complete chain of explicitly retained,
/// non-copying array-growth stubs ending at the exact live ordinary array the
/// verifier found. Nursery aliases and any intervening evacuation stub decline.
/// Does not allocate, modify the chain, or change collection policy.
pub(crate) fn is_retained_growth_alias(old_bits: u64, new_bits: u64) -> bool {
    fn array_addr(bits: u64) -> Option<usize> {
        match bits >> 48 {
            0 => Some(bits as usize),
            0x7ffd => Some((bits & crate::value::POINTER_MASK) as usize),
            _ => None,
        }
    }
    let (Some(mut address), Some(expected)) = (array_addr(old_bits), array_addr(new_bits)) else {
        return false;
    };
    if address == expected {
        return false;
    }
    for _ in 0..64 {
        let Some(header) =
            (unsafe { crate::value::addr_class::try_read_tracked_gc_header(address) })
        else {
            return false;
        };
        if !matches!(
            crate::arena::classify_heap_generation(address),
            crate::arena::HeapGeneration::Old | crate::arena::HeapGeneration::Longlived
        ) {
            return false;
        }
        unsafe {
            let header = header.as_ref();
            if header.obj_type != crate::gc::GC_TYPE_ARRAY {
                return false;
            }
            if header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0 {
                let array = &*(address as *const super::ArrayHeader);
                return address == expected
                    && header._reserved & crate::gc::GC_LAYOUT_STATE_MASK != GROWTH_STUB_LAYOUT
                    && array.length <= array.capacity
                    && (array.capacity as usize)
                        <= (header.size as usize).saturating_sub(
                            crate::gc::GC_HEADER_SIZE + std::mem::size_of::<super::ArrayHeader>(),
                        ) / 8;
            }
            if header._reserved & crate::gc::GC_LAYOUT_STATE_MASK != GROWTH_STUB_LAYOUT {
                return false;
            }
            let next = crate::gc::forwarding_address(header) as usize;
            if next == address {
                return false;
            }
            address = next;
        }
    }
    false
}
