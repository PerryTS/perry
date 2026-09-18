//! The per-class guard expectation: the poisonable carrier that replaced the
//! `@PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED` latch on the per-access
//! class-field guard.
//!
//! # Why a second global, beside the ShapeId
//!
//! The obvious move is to poison `@perry_class_shape_id_*` itself — the guard
//! already loads it, so disabling the fast path would cost nothing. That is
//! UNSOUND: `js_object_alloc_class_inline_keys_stamped` stamps every newly
//! allocated instance with the value read out of that global
//! (`perry-codegen/src/lower_call/new_alloc.rs`), so poisoning it would brand
//! live objects with a bogus ShapeId rather than close a fast path.
//!
//! `@perry_class_guard_shape_*` is seeded with the same ShapeId at module init
//! and is only ever COMPARED against, never stamped — so it is safe to poison,
//! and the guard gets the latch's authority out of a load it was making
//! anyway. Net on the emitted fast path of a static-key read: 28 -> 23 ARM64
//! instructions, and one fewer dependent load (the latch was an `external
//! global`, so reading it cost a GOT load plus an `ldrb` through it).

use super::descriptor_state::PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED;
use std::sync::atomic::Ordering;

/// The value written into every registered `@perry_class_guard_shape_*` slot
/// when the inline fast path is disabled.
///
/// ShapeIds are allocated from `[SHAPE_ID_BASE, SHAPE_ID_END)` =
/// `[0x8000_0000, 0xC000_0000)` and are never reused (`object/shapes.rs`), so
/// `u32::MAX` can never be a live ShapeId. A guard comparing an object's
/// `class_id | ShapeId << 32` word against a poisoned expectation therefore
/// misses for EVERY receiver, which is exactly what the latch bought — at no
/// per-access cost, because the guard already had to load its expectation.
pub const CLASS_GUARD_SHAPE_POISON: u32 = u32::MAX;

/// Addresses of the per-class guard-expectation slots compiled code emits.
///
/// Held as `(usize, u32)` — address plus the ShapeId it was seeded with — and
/// never as `*mut u32`: these point into the program's own data segment, never
/// into the Perry heap, so this table is deliberately NOT a GC root holder and
/// must not be registered with `gc_register_mutable_root_scanner`. The seeded
/// value is kept so `test_reset_class_field_inline_guard` can restore it;
/// production never unpoisons (the decision is monotonic).
static CLASS_GUARD_SHAPE_SLOTS: std::sync::Mutex<Vec<(usize, u32)>> =
    std::sync::Mutex::new(Vec::new());

/// Register a compiled module's per-class guard-expectation slot.
///
/// Called once per class from module init, right after the slot is seeded with
/// the class's freshly minted ShapeId. A module initialised AFTER the latch
/// already flipped is poisoned on the spot, so late `require`/`dlopen` arrivals
/// cannot reopen a fast path the process has already closed.
///
/// # Safety
/// `slot` must be a valid, writable, 4-byte-aligned `u32` with static lifetime
/// — i.e. a `@perry_class_guard_shape_*` global emitted by perry-codegen.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_guard_shape(slot: *mut u32) {
    if slot.is_null() {
        return;
    }
    if let Ok(mut slots) = CLASS_GUARD_SHAPE_SLOTS.lock() {
        // SAFETY: caller contract above.
        let seeded = unsafe { slot.read() };
        slots.push((slot as usize, seeded));
        if PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED.load(Ordering::Relaxed) != 0 {
            // SAFETY: caller contract above.
            unsafe { slot.write(CLASS_GUARD_SHAPE_POISON) };
        }
    }
}

pub(super) fn poison_class_guard_shapes() {
    if let Ok(slots) = CLASS_GUARD_SHAPE_SLOTS.lock() {
        for &(addr, _) in slots.iter() {
            // SAFETY: every entry was registered through
            // `js_register_class_guard_shape`, whose contract requires a valid
            // writable static `u32`.
            unsafe { (addr as *mut u32).write(CLASS_GUARD_SHAPE_POISON) };
        }
    }
}

/// Restore every registered expectation to the ShapeId it was seeded with.
///
/// Production never does this — the disable decision is monotonic — but a test
/// that flips the latch must not leave later tests guarding against
/// [`CLASS_GUARD_SHAPE_POISON`].
pub(super) fn restore_class_guard_shapes_for_test() {
    if let Ok(slots) = CLASS_GUARD_SHAPE_SLOTS.lock() {
        for &(addr, seeded) in slots.iter() {
            // SAFETY: registered through `js_register_class_guard_shape`.
            unsafe { (addr as *mut u32).write(seeded) };
        }
    }
}
