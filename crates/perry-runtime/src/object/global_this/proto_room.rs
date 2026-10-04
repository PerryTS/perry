//! A builtin prototype is born wide enough to hold its own keys inline.
//!
//! Every builtin prototype (`%Object.prototype%`, `Map.prototype`, ...) gains
//! its keys by name right after it is allocated: `constructor`, then what its
//! installer adds. Born with the floor's two slots, everything past them went
//! to the spill buffer, and a method site cannot use a spill-located holder
//! slot (its inherited entry loads `[holder + HDR + 8*i]`). So `o.toString()`
//! and `o.hasOwnProperty(k)` on a plain object never got a site entry.
//!
//! Instead the prototype is born with [`BUILTIN_PROTOTYPE_ROOM`] live inline
//! slots, the installer's ordinary key adds fill them in order, and once its
//! own keys are in, [`fit_builtin_prototype`] publishes the bound it actually
//! uses and returns the unused tail of the allocation. Which keys are inline
//! is then what it is for any object: a fact of its ShapeId (the live inline
//! bound), read by every path the same way. There is no per-builtin count and
//! no table: a prototype with more keys than the room keeps the rest in the
//! spill buffer, exactly as before.

use super::*;

/// Inline slots a builtin prototype is born with: room, not a count. Large
/// enough for every prototype a plain object can inherit from directly
/// (`%Object.prototype%` holds 8 keys, `RegExp.prototype` about 30); one with
/// more keeps the excess in the spill buffer.
pub(crate) const BUILTIN_PROTOTYPE_ROOM: u32 = 64;

/// A fresh builtin prototype object with [`BUILTIN_PROTOTYPE_ROOM`] inline
/// slots. Its caller installs its keys and then calls
/// [`fit_builtin_prototype`].
pub(crate) fn alloc_builtin_prototype() -> *mut ObjectHeader {
    js_object_alloc(0, BUILTIN_PROTOTYPE_ROOM)
}

per_test_global! {
    static FITTED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
}

/// Builtin prototypes whose unused room was returned (diagnostic, tests).
#[cfg(test)]
pub(crate) fn builtin_prototypes_fitted() -> u64 {
    FITTED.load(Ordering::Relaxed)
}

/// Shrink a builtin prototype born by [`alloc_builtin_prototype`] to the
/// inline slots its keys use: publish that live bound (a ShapeId transition
/// that moves no slot), then cut the allocation to it and leave the tail as a
/// dead header (`obj_type == 0`, `size` intact), which every arena walker
/// skips. The tail is only cut in the nursery, where the next minor copies
/// the object at its new size; anywhere else the bound alone shrinks.
///
/// # Safety
/// `obj` is a live prototype from [`alloc_builtin_prototype`] that has not
/// been exposed to script yet, with collection suppressed by the caller.
pub(crate) unsafe fn fit_builtin_prototype(obj: *mut ObjectHeader) {
    if obj.is_null() {
        return;
    }
    let Some(shape) = crate::object::shapes::object_shape_descriptor(obj) else {
        return;
    };
    let room = shape.live_inline_slot_count;
    let used = shape
        .logical_key_count
        .min(room)
        .max(crate::object::INLINE_SLOT_FLOOR as u32);
    if used >= room {
        return;
    }
    crate::object::shapes::publish_object_live_slot_count(obj, used);
    if crate::object::shapes::object_shape_descriptor(obj)
        .map_or(true, |d| d.live_inline_slot_count != used)
    {
        return;
    }
    FITTED.fetch_add(1, Ordering::Relaxed);
    if !crate::arena::pointer_in_nursery(obj as usize) {
        return;
    }
    let header = (obj as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
    let total = (*header).size as usize;
    let fitted = crate::gc::GC_HEADER_SIZE
        + std::mem::size_of::<ObjectHeader>()
        + used as usize * std::mem::size_of::<u64>();
    if fitted + crate::gc::GC_HEADER_SIZE > total {
        return;
    }
    // The dead tail first, then the object's own size: a walker between the
    // two stores still hops over a well-formed run.
    let tail = (header as *mut u8).add(fitted) as *mut crate::gc::GcHeader;
    // GC_STORE_AUDIT(INIT): the cut tail of a fresh, unexposed allocation
    // becomes a dead header; it holds no reference.
    std::ptr::write(
        tail,
        crate::gc::GcHeader {
            obj_type: 0,
            gc_flags: 0,
            _reserved: 0,
            size: crate::gc::gc_header_size_word(total - fitted),
        },
    );
    (*header).size = crate::gc::gc_header_size_word(fitted);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `%Object.prototype%` holds its own keys inline (the method site's
    /// inherited entry reads them there), its live bound is exactly what they
    /// use, and the allocation's unused room went back.
    #[test]
    fn object_prototype_keys_are_inline_and_the_room_is_returned() {
        std::thread::Builder::new()
            .stack_size(16 << 20)
            .spawn(|| unsafe {
                let (_, proto) = ensure_object_intrinsics();
                assert!(!proto.is_null());
                assert!(builtin_prototypes_fitted() > 0, "no prototype was fitted");
                let d = crate::object::shapes::object_shape_descriptor(proto).expect("shape");
                let keys = d.logical_key_count;
                assert!(keys >= 7, "%Object.prototype% has {keys} keys");
                assert_eq!(
                    d.live_inline_slot_count,
                    keys.max(crate::object::INLINE_SLOT_FLOOR as u32)
                );
                let list = d.keys as usize as *const crate::array::ArrayHeader;
                for name in ["constructor", "toString", "hasOwnProperty", "valueOf"] {
                    let s = crate::object::keys_find_slot_by_bytes_resolved(
                        list,
                        keys,
                        name.as_bytes(),
                    )
                    .unwrap_or_else(|| panic!("{name} missing"));
                    assert!(
                        s < d.live_inline_slot_count,
                        "{name} at slot {s} is not inline"
                    );
                }
                if crate::arena::pointer_in_nursery(proto as usize) {
                    let header = (proto as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                        as *const crate::gc::GcHeader;
                    assert_eq!(
                        (*header).size as usize,
                        crate::gc::GC_HEADER_SIZE
                            + std::mem::size_of::<ObjectHeader>()
                            + d.live_inline_slot_count as usize * 8
                    );
                    let tail = (header as *const u8).add((*header).size as usize)
                        as *const crate::gc::GcHeader;
                    assert_eq!((*tail).obj_type, 0, "the returned room is a dead header");
                }
            })
            .expect("spawn")
            .join()
            .expect("object-prototype fit test panicked");
    }
}
