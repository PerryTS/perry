//! Array forwarding (#233): the chain walk every array resolver shares, and
//! the resolver for arrays a runtime structure owns (a shape's keys array),
//! which follows forwarding and nothing else (#12743).

use super::ArrayHeader;

/// Follow `cleaned`'s array forwarding chain (#233) to the live array head,
/// returning it with its tracked header (`None` header: an untracked
/// address, returned unchanged). `None` for a corrupt chain: a target that is
/// not a tracked array, or more than 64 edges.
///
/// # Safety
/// `tracked_header` must be `cleaned`'s tracked GC header, if any.
#[inline]
pub(super) unsafe fn follow_array_forwarding(
    mut cleaned: *const ArrayHeader,
    mut tracked_header: Option<std::ptr::NonNull<crate::gc::GcHeader>>,
) -> Option<(
    *const ArrayHeader,
    Option<std::ptr::NonNull<crate::gc::GcHeader>>,
)> {
    unsafe {
        let mut steps = 0u32;
        let mut first_forwarded_header: *mut crate::gc::GcHeader = std::ptr::null_mut();
        while let Some(gc_header) = tracked_header {
            let gc_header = gc_header.as_ptr();
            if (*gc_header).obj_type != crate::gc::GC_TYPE_ARRAY
                || (*gc_header).gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
            {
                break;
            }
            if steps == 0 {
                first_forwarded_header = gc_header;
            }
            let new_user = crate::gc::forwarding_address(gc_header) as usize;
            let Some(target_header) =
                crate::value::addr_class::try_read_tracked_gc_header(new_user)
            else {
                return None;
            };
            if (*target_header.as_ptr()).obj_type != crate::gc::GC_TYPE_ARRAY {
                return None;
            }
            cleaned = new_user as *const ArrayHeader;
            tracked_header = Some(target_header);
            steps += 1;
            if steps > 64 {
                return None;
            }
        }
        // A receiver stored through an alias can keep its original array head
        // across several capacity-crossing grows. Generated array guards heal
        // one forwarding edge inline; without compression, a two-or-more-edge
        // chain therefore rejects the guard on every element access and pays
        // this entire resolver repeatedly. Once the validated walk reaches the
        // live array, point the original retained stub straight at that head.
        // The next generated access can then heal its single edge inline.
        //
        // Do this only after walking and validating the complete chain. A
        // corrupt target/cycle returns above and must not rewrite a stub with
        // an address that has not been proved to be a tracked array.
        if steps > 1 && !first_forwarded_header.is_null() {
            crate::gc::set_forwarding_address(
                first_forwarded_header,
                cleaned as *mut ArrayHeader as *mut u8,
            );
        }
    }
    Some((cleaned, tracked_header))
}

/// Resolve an array a runtime structure owns (a shape's keys array) to its
/// live head. Such an array is a dense `GC_TYPE_ARRAY` by construction: it
/// is never a lazy JSON array, a Buffer or a sparse array, so unlike
/// [`super::clean_arr_ptr`] this follows forwarding and nothing else. In
/// particular it never materializes a lazy array, which runs the generic
/// property set (user setters, a throw) and would make every keys walk a
/// call that can run JS (#12743). Null for anything that is not a tracked
/// array.
pub(crate) fn clean_owned_arr_ptr(arr: *const ArrayHeader) -> *const ArrayHeader {
    let bits = arr as u64;
    let cleaned = if bits >> 48 == crate::value::POINTER_TAG >> 48 {
        (bits & crate::value::POINTER_MASK) as *const ArrayHeader
    } else if bits >> 48 == 0 {
        arr
    } else {
        return std::ptr::null();
    };
    if !crate::value::addr_class::is_plausible_heap_addr(cleaned as usize) {
        return std::ptr::null();
    }
    unsafe {
        let tracked = crate::value::addr_class::try_read_tracked_gc_header(cleaned as usize);
        match follow_array_forwarding(cleaned, tracked) {
            Some((live, Some(header)))
                if (*header.as_ptr()).obj_type == crate::gc::GC_TYPE_ARRAY =>
            {
                live
            }
            _ => std::ptr::null(),
        }
    }
}
