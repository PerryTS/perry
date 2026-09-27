//! The full-outline generic property read, split into a GC-leaf hit and a
//! collecting miss (deferred-collection RFC, step S2).
//!
//! # Why this exists
//!
//! In a module over the full-outline threshold (#5391 path 3) every generic
//! `obj.prop` read is ONE call to [`js_object_get_field_ic`]. That helper can
//! run a getter, a Proxy trap or an allocating miss, so the call is an RS4GC
//! statepoint: every GC value live across it is spilled before the call and
//! reloaded after, on every read, including the ~95 % that are monomorphic
//! hits answered from the site's MRU word (`pic_outlined_mru_hit`). On the
//! claude-code bundle that is 191,964 call sites.
//!
//! The split moves the hit into [`js_object_get_field_ic_fast`], a call that
//! reaches no collector entry, so codegen marks it `"gc-leaf-function"` and
//! RS4GC leaves it alone. Only its decline arm calls
//! [`js_object_get_field_ic_fast_miss`], which is still a statepoint.
//!
//! # Why the fast entry is a Perry-GC leaf on today's runtime
//!
//! Its whole call graph, read at the source (the S1 call-graph checker is the
//! authority; this is what it has to agree with):
//!
//! * a tag compare and a mask on the receiver bits;
//! * `pic_outlined_mru_hit`: `outlined_mru_hit_enabled` (a `OnceLock<bool>`
//!   whose first call reads one environment variable into Rust-heap memory,
//!   which cannot arm a Perry trigger), the handle-band compare,
//!   `pic_slot_peek` (one acquire load, never `pic_slot_resolve`, which is the
//!   allocating variant), `object_shape_stamp` (one header load), two cache
//!   word compares and one slot load;
//! * `typed_feedback_active` (one static read).
//!
//! It makes NO typed-feedback call. With feedback on, the ladder's observe and
//! guard-pass record take the feedback registry lock, a `GcRootRegistryGuard`
//! whose drop can flush a deferred collection request (#11523) — the census
//! call graph reaches the collector exactly there. So under feedback the fast
//! entry declines every read and the continuation records it; with feedback
//! off those two calls are early returns, and skipping them changes nothing.
//!
//! No Perry allocation, no `GcRootRegistryGuard`, no throw, no call into
//! generated code, no poll. Everything it cannot serve — a non-POINTER
//! receiver (SSO, class ref, nullish, primitive, heap string), an unprimed or
//! mismatched MRU word, an overflow slot, the Array-subclass word, a
//! polymorphic way — answers `TAG_HOLE`.
//!
//! # Why the pair is behaviourally identical to the one call
//!
//! `TAG_HOLE` is unambiguous: #10826 makes every delete a ShapeId transition,
//! so a stamp hit never reads a hole. A hit is served only with feedback off,
//! where the ladder's two feedback calls are no-ops; the miss continuation is
//! the unchanged ladder with only the MRU probe skipped when the fast entry
//! already asked it (feedback off). A receiver the fast entry declined for its
//! tag never reached the MRU probe in the old ladder either.

use super::ic_miss::{get_field_ic_dispatch, pic_outlined_mru_hit};
use crate::object::{ObjectHeader, PicCacheSlot};

/// The GC-leaf hit of the full-outline generic read. Answers the slot value on
/// an MRU hit and `TAG_HOLE` for everything else; the caller then calls
/// [`js_object_get_field_ic_fast_miss`] with the same operands.
///
/// Same operands as [`super::js_object_get_field_ic`], so the emitted miss
/// arm passes them through unchanged.
#[no_mangle]
pub extern "C" fn js_object_get_field_ic_fast(
    obj_bits: i64,
    key: *const crate::StringHeader,
    site_id: u64,
    cache_slot: *mut PicCacheSlot,
) -> f64 {
    let _ = (key, site_id);
    if crate::typed_feedback::typed_feedback_active() {
        return f64::from_bits(crate::value::TAG_HOLE);
    }
    mru_hit_or_hole(obj_bits, cache_slot)
}

/// The fast entry's hit, without the feedback gate (unit tests run with
/// feedback forced on).
#[inline(always)]
fn mru_hit_or_hole(obj_bits: i64, cache_slot: *mut PicCacheSlot) -> f64 {
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
    let bits = obj_bits as u64;
    // POINTER tag only, exactly as the full ladder admits the MRU probe
    // (#10833): a heap STRING's `+4` is not a ShapeId word.
    if bits >> 48 != 0x7FFD {
        return f64::from_bits(crate::value::TAG_HOLE);
    }
    let obj_handle = (bits & POINTER_MASK) as usize as *const ObjectHeader;
    match unsafe { pic_outlined_mru_hit(obj_handle, cache_slot) } {
        Some(value) => value,
        None => f64::from_bits(crate::value::TAG_HOLE),
    }
}

/// The collecting miss continuation of [`js_object_get_field_ic_fast`]: the
/// complete full-outline ladder minus the MRU probe the fast entry already
/// made. A statepoint, like the single call it replaces.
#[no_mangle]
pub extern "C" fn js_object_get_field_ic_fast_miss(
    obj_bits: i64,
    key: *const crate::StringHeader,
    site_id: u64,
    cache_slot: *mut PicCacheSlot,
) -> f64 {
    // Under feedback the fast entry asked nothing, so the probe runs here
    // (and records its observe and guard pass, as the single helper did).
    let probe_mru = crate::typed_feedback::typed_feedback_active();
    get_field_ic_dispatch(obj_bits, key, site_id, cache_slot, probe_mru)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{PicCache, PIC_CACHE_WORDS};

    fn key_of(bytes: &[u8]) -> *const crate::StringHeader {
        crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
    }

    fn boxed(obj: *mut ObjectHeader) -> i64 {
        (0x7FFD_u64 << 48 | obj as u64) as i64
    }

    /// The pair answers what the single helper answers, and the fast entry
    /// declines exactly the reads it cannot serve from the MRU word.
    #[test]
    fn fast_hit_and_miss_continuation_match_the_single_helper() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 4));
        let k = scope.root_string_ptr(key_of(b"split_present"));
        obj.with_mut_ptr(|o| {
            k.with_const_ptr(|kp| crate::object::js_object_set_field_by_name(o, kp, 7.0))
        });
        let mut cache: PicCache = [0; PIC_CACHE_WORDS];
        let mut slot: PicCacheSlot = &mut cache;
        let slot_ptr: *mut PicCacheSlot = &mut slot;

        obj.with_mut_ptr(|o| {
            k.with_const_ptr(|kp| {
                // Unit tests run with typed feedback forced ON, where the
                // fast entry must decline every read (its hit would skip the
                // observe the ladder records) and the continuation answers.
                assert!(crate::typed_feedback::typed_feedback_active());
                let declined = js_object_get_field_ic_fast(boxed(o), kp, 0, slot_ptr);
                assert_eq!(declined.to_bits(), crate::value::TAG_HOLE);
                assert_eq!(
                    js_object_get_field_ic_fast_miss(boxed(o), kp, 0, slot_ptr),
                    7.0
                );
                // The hit itself (the feedback-off path): primed by the
                // continuation above, served from the MRU word.
                assert_eq!(
                    mru_hit_or_hole(boxed(o), slot_ptr),
                    7.0,
                    "a primed monomorphic read must be served by the leaf hit"
                );
                assert_eq!(
                    super::super::js_object_get_field_ic(boxed(o), kp, 0, slot_ptr),
                    7.0
                );
                // Non-POINTER receivers never reach the probe.
                for recv in [
                    crate::value::TAG_UNDEFINED as i64,
                    crate::value::TAG_NULL as i64,
                    (0x7FFE_u64 << 48 | 3) as i64,
                    1.5f64.to_bits() as i64,
                ] {
                    assert_eq!(
                        mru_hit_or_hole(recv, slot_ptr).to_bits(),
                        crate::value::TAG_HOLE
                    );
                }
            })
        });
    }
}
