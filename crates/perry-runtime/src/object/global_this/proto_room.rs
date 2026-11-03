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
//! bound), read by every path the same way. Allocation room is not a lookup
//! fact: small fixed installers can request less room, and a prototype with
//! more keys than its room keeps the rest in the spill buffer, as before.

use super::*;

/// Inline slots a builtin prototype is born with: room, not a count. Large
/// enough for every prototype a plain object can inherit from directly
/// (`%Object.prototype%` holds 8 keys, `RegExp.prototype` about 30); one with
/// more keeps the excess in the spill buffer.
pub(crate) const BUILTIN_PROTOTYPE_ROOM: u32 = 64;

/// A fresh builtin prototype object with [`BUILTIN_PROTOTYPE_ROOM`] inline
/// slots. Its caller installs its keys and then calls
/// [`fit_builtin_prototype`].
///
/// Born old and never-move (owner decision O3, `gc::never_move`): every site
/// memo that answers from a builtin prototype (`%Object.prototype%` above all,
/// the terminal of most absent reads) is admitted from the first prime.
pub(crate) fn alloc_builtin_prototype() -> *mut ObjectHeader {
    alloc_builtin_prototype_with_room(BUILTIN_PROTOTYPE_ROOM)
}

/// The same intrinsic birth with room supplied by a small fixed installer.
/// This is allocation capacity only; ordinary key installs and the final
/// fit publish the shape that proves which slots are live and inline.
pub(crate) fn alloc_builtin_prototype_with_room(room: u32) -> *mut ObjectHeader {
    crate::object::object_alloc_prototype_old(0, room)
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
    if !crate::arena::pointer_in_nursery(obj as usize) {
        return;
    }
    let header = (obj as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
    let total = (*header).size as usize;
    let fitted =
        crate::gc::GC_HEADER_SIZE + super::super::header::meta_form_alloc_bytes(used as usize);
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

    /// `%Array.prototype%`'s named holder (its property bag) is born old and
    /// never-move like every builtin prototype, so an array method site's
    /// memo admits it before any minor has run.
    #[test]
    fn array_prototype_bag_is_born_old_and_never_move() {
        std::thread::Builder::new()
            .stack_size(16 << 20)
            .spawn(|| unsafe {
                crate::object::js_get_global_this();
                let proto = crate::array::array_prototype_addr();
                assert_ne!(proto, 0, "no %Array.prototype%");
                let bag =
                    crate::array::array_property_bag(proto as *const crate::array::ArrayHeader);
                assert!(!bag.is_null(), "%Array.prototype% has no property bag");
                let header =
                    (bag as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                // Never-move from birth.
                assert!(crate::gc::never_move::is_never_move(header));
                let d = crate::object::shapes::object_shape_descriptor(bag).expect("shape");
                let list = d.keys_view();
                for name in ["constructor", "slice", "toString"] {
                    let s = crate::object::keys_find_slot_by_bytes_resolved(
                        list,
                        d.logical_key_count,
                        name.as_bytes(),
                    )
                    .unwrap_or_else(|| panic!("{name} missing"));
                    assert!(s < d.live_inline_slot_count, "{name} is not inline");
                }
            })
            .expect("spawn")
            .join()
            .expect("array-prototype bag test panicked");
    }

    /// `%Object.prototype%` holds its own keys inline (the method site's
    /// inherited entry reads them there) and its live bound is exactly what
    /// they use: it is born on its static layout, with no room to return.
    #[test]
    fn object_prototype_keys_are_inline_and_the_room_is_returned() {
        std::thread::Builder::new()
            .stack_size(16 << 20)
            .spawn(|| unsafe {
                let (_, proto) = ensure_object_intrinsics();
                assert!(!proto.is_null());
                let d = crate::object::shapes::object_shape_descriptor(proto).expect("shape");
                let keys = d.logical_key_count;
                assert!(keys >= 7, "%Object.prototype% has {keys} keys");
                assert_eq!(
                    d.live_inline_slot_count,
                    keys.max(crate::object::INLINE_SLOT_FLOOR as u32)
                );
                let list = d.keys_view();
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
                            + super::super::header::meta_form_alloc_bytes(
                                d.live_inline_slot_count as usize
                            )
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
