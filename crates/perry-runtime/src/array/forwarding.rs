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
            if crate::gc_type_is_not_array!((*gc_header).obj_type)
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
            if crate::gc_type_is_not_array!((*target_header.as_ptr()).obj_type) {
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
            Some((live, Some(header))) if crate::gc_type_is_array!((*header.as_ptr()).obj_type) => {
                live
            }
            _ => std::ptr::null(),
        }
    }
}

/// Array growth stubs born so far, process-wide (#13792). A key stored in an
/// identity table becomes a forwarding stub only through a growth after it was
/// stored, so a table that proved at epoch E that it holds no stub key still
/// holds none while the epoch is E. One counter for every thread: a growth on
/// another thread only withdraws proofs early, and a plain static is one load
/// on the miss path where a thread-local is a TLS lookup.
static ARRAY_GROWTH_EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// The current array growth epoch (never 0).
#[inline(always)]
pub(crate) fn array_growth_epoch() -> u64 {
    ARRAY_GROWTH_EPOCH.load(std::sync::atomic::Ordering::Relaxed)
}

/// A growth stub was installed: every "no stub key" proof is stale. An atomic
/// add, so the epoch never repeats a value a proof could have stamped.
#[inline]
pub(in crate::array) fn note_array_growth_stub() {
    ARRAY_GROWTH_EPOCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// Whether two `POINTER_TAG` values name the same array (#13792). Growth
/// reallocates an array and leaves a forwarding stub at its old head, so one
/// array has two addresses until a collection heals every reference: a
/// binding refreshed after the grow holds the new head, an alias taken
/// before it (or a key a table stored) holds the stub. Identity is the
/// forwarding-resolved head, as `===` (`value::resolve_forwarding`) defines
/// it. Every identity-keyed table compares through this once raw bits miss.
/// False for anything that is not two arrays.
pub(crate) fn arrays_share_identity(a_bits: u64, b_bits: u64) -> bool {
    if a_bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG
        || b_bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG
    {
        return false;
    }
    let a = (a_bits & crate::value::POINTER_MASK) as usize;
    let b = (b_bits & crate::value::POINTER_MASK) as usize;
    unsafe {
        let (Some(ha), Some(hb)) = (
            crate::value::addr_class::try_read_gc_header(a),
            crate::value::addr_class::try_read_gc_header(b),
        ) else {
            return false;
        };
        if crate::gc_type_is_not_array!(ha.obj_type) || crate::gc_type_is_not_array!(hb.obj_type) {
            return false;
        }
        if a == b {
            return true;
        }
        // A second name only exists through a forwarding stub: two distinct
        // addresses neither of which is forwarded are two arrays. This keeps
        // the chain walk off every miss of a table probe.
        if (ha.gc_flags | hb.gc_flags) & crate::gc::GC_FLAG_FORWARDED == 0 {
            return false;
        }
    }
    // Forwarding only, never `clean_arr_ptr`: identity must not materialize a
    // lazy array (that runs the generic property set) inside a table probe.
    crate::value::resolve_forwarding(a) == crate::value::resolve_forwarding(b)
}

/// Whether `bits` is a `POINTER_TAG` array: the one key kind that growth
/// forwarding gives a second address (#13792). Identity tables hash such a key
/// by its relocation-stable token instead of its address.
#[inline]
pub(crate) fn is_array_value_bits(bits: u64) -> bool {
    bits & !crate::value::POINTER_MASK == crate::value::POINTER_TAG
        && unsafe {
            crate::value::addr_class::try_read_gc_header((bits & crate::value::POINTER_MASK) as usize)
        }
        .is_some_and(|h| crate::gc_type_is_array!(h.obj_type))
}

/// The relocation-stable identity token of an array value (#13792): the one
/// Set keys use (`set::key_identity`, stored on the array's property bag
/// meta, which growth and collection both carry along). `create` allocates
/// one if missing (insertion); a probe passes `false`, and `None` then means
/// the array was never inserted into any identity table, a definite miss.
pub(crate) fn array_identity_token(bits: u64, create: bool) -> Option<u64> {
    let addr = crate::value::resolve_forwarding((bits & crate::value::POINTER_MASK) as usize);
    unsafe { crate::set::key_identity(addr, create) }
}
