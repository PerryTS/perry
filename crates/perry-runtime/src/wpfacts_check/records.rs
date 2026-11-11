//! Check mode for value records (DESIGN-p4l4 B7, L4; VR-2).
//!
//! Under `PERRY_VERIFY_FACTS=1` with `PERRY_FACTS_VALUE_RECORDS=1` the
//! compiler claims every instance of a template the facts admit as a value
//! record at its birth (`perry_verify_facts_record_claim`: the OBJECT header
//! bit `OBJ_FLAG_RECORD_CLAIM`). The facts say such an object's identity is
//! never observed (R4): compiled `===`/`==`/`switch` call
//! `perry_verify_facts_record_identity` with both operands, which aborts with
//! rc 86 when one carries the claim.
//!
//! The runtime's own identity routes (`value/equality.rs`, Map and Set keys)
//! are not hooked: even a flag test there costs release code (it moved chess
//! search by 2.2 M instructions), so they wait for VR-3's copies, which a
//! check-mode materializer can mark. A release program never sets the bit.
use super::*;
use std::sync::atomic::Ordering;

/// The OBJECT header of `bits`, when it is one.
fn object_header(bits: u64) -> Option<*mut crate::gc::GcHeader> {
    let bits = crate::value::equality::normalize_raw_object_bits(bits);
    if bits & TAG_MASK != POINTER_TAG {
        return None;
    }
    let addr = (bits & POINTER_MASK) as usize;
    unsafe {
        crate::value::addr_class::try_read_tracked_gc_header_of_type(
            addr,
            crate::gc_object_type_pat!(),
        )
    }
    .map(|h| h.as_ptr())
}

/// The value carries the record claim.
pub(crate) fn claimed(bits: u64) -> bool {
    object_header(bits)
        .is_some_and(|h| unsafe { (*h)._reserved } & crate::gc::OBJ_FLAG_RECORD_CLAIM != 0)
}

/// Claim a freshly born instance of an admitted record template.
#[no_mangle]
pub extern "C" fn perry_verify_facts_record_claim(value: f64) {
    if let Some(h) = object_header(value.to_bits()) {
        unsafe { (*h)._reserved |= crate::gc::OBJ_FLAG_RECORD_CLAIM };
    }
}

/// A compiled identity comparison (`===`, `!==`, `==`, `!=`, a `switch`
/// case) of `a` and `b`. Returns 0; aborts when either is a claimed record.
#[no_mangle]
pub extern "C" fn perry_verify_facts_record_identity(
    a: f64,
    b: f64,
    site: i64,
    site_len: i64,
) -> i32 {
    CHECKS.fetch_add(1, Ordering::Relaxed);
    for v in [a.to_bits(), b.to_bits()] {
        if claimed(v) {
            // The site metadata carries the predicate (`site_arg`).
            Witness::raw(site, site_len)
                .receiver(v)
                .fail_static("a value record's identity is compared".into())
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unclaimed_value_passes_and_a_claimed_one_is_seen() {
        // Primitives are never claimed.
        assert!(!claimed(1.5f64.to_bits()));
        assert!(!claimed(crate::value::TAG_UNDEFINED));
        let o = crate::object::js_object_alloc(0, 0);
        let boxed = POINTER_TAG | (o as usize as u64 & POINTER_MASK);
        assert!(!claimed(boxed));
        perry_verify_facts_record_claim(f64::from_bits(boxed));
        assert!(claimed(boxed));
    }
}
