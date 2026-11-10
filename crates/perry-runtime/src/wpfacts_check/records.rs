//! Check mode for value records (DESIGN-p4l4 B7, L4; VR-2).
//!
//! Under `PERRY_VERIFY_FACTS=1` with `PERRY_FACTS_VALUE_RECORDS=1` the
//! compiler claims every instance of a template the facts admit as a value
//! record at its birth (`perry_verify_facts_record_claim`: the OBJECT header
//! bit `OBJ_FLAG_RECORD_CLAIM`). The facts say such an object's identity is
//! never observed (R4): compiled `===`/`==`/`switch` call
//! `perry_verify_facts_record_identity` with both operands, and the runtime's
//! identity routes (`value/equality.rs`, Map and Set keys) call [`route`].
//! Either aborts with rc 86 when an operand carries the claim.
//!
//! A release program never sets the bit and never arms the routes: [`route`]
//! is one relaxed load of `ARMED` (set by the first claim) on paths that
//! compare two values.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

static ARMED: AtomicBool = AtomicBool::new(false);

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
        ARMED.store(true, Ordering::Relaxed);
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
            Witness::raw(site, site_len).receiver(v).fail(
                "value record identity",
                "a value record's identity is compared".into(),
            )
        }
    }
    0
}

/// A runtime identity route (`what`) comparing `a` and `b`.
#[inline(always)]
pub(crate) fn route(a: u64, b: u64, what: &'static str) {
    if ARMED.load(Ordering::Relaxed) {
        route_slow(a, b, what);
    }
}

#[cold]
#[inline(never)]
fn route_slow(a: u64, b: u64, what: &'static str) {
    if claimed(a) || claimed(b) {
        let line = format!(
            "{{\"marker\":\"PERRY_FALSE_PROOF\",\"predicate\":\"value record identity\",\"route\":\"{what}\"}}\n"
        );
        unsafe {
            libc::write(2, line.as_ptr().cast(), line.len() as _);
            libc::_exit(86)
        }
    }
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
        let boxed = POINTER_TAG | (o as u64 & POINTER_MASK);
        assert!(!claimed(boxed));
        perry_verify_facts_record_claim(f64::from_bits(boxed));
        assert!(claimed(boxed));
        assert!(ARMED.load(Ordering::Relaxed));
        // An unclaimed pair passes the armed route.
        route(1.0f64.to_bits(), 2.0f64.to_bits(), "test");
    }
}
