//! The typed-shape layout installation protocol: how a constructed object's
//! canonical raw-f64 / pointer slot descriptor is declared
//! (pre-constructor, `js_gc_declare_typed_shape_layout`) or validated
//! (post-constructor, `js_gc_init_typed_shape_layout`) and then installed —
//! shared by shape via `gc::shape_install`, else per object.
//!
//! Split out of `gc/layout.rs` for the 2000-line file cap (#8204 took it to
//! 2110). Pure code move — no logic change.

use crate::gc::shape_install;

use super::*;

/// How `init_typed_shape_layout` establishes the descriptor's truth.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TypedShapeProof {
    /// **Observe.** The object's fields already hold their final values, so
    /// check every one against the declared masks and refuse the descriptor if
    /// any disagrees. This is the post-constructor call site.
    ValidateSlots,
    /// **Construct.** The object was allocated moments ago and its slots are
    /// still the allocator's `TAG_UNDEFINED` fill, so there is nothing to
    /// observe yet — validating would reject every raw-f64 slot (`undefined`
    /// carries a `0x7FFC` tag, inside `layout_raw_f64_bits`' reject range) and
    /// downgrade the object it was asked to describe.
    ///
    /// The caller carries the proof instead, and it is a codegen one: #7510
    /// emits this form only for a class whose constructor prologue provably
    /// assigns **every** raw-f64 field from a plain parameter before any other
    /// statement runs (`lower_call::field_init`'s #7486 predicate), so no read
    /// can observe a raw-f64 slot between here and its first write.
    ///
    /// The *collector's* half needs no proof at all: `TAG_UNDEFINED` is a
    /// non-pointer in every slot, which is consistent with both the
    /// `POINTER_FREE` and the `SIDE_MASK` state this installs.
    ///
    /// And the descriptor stays honest afterwards without the loop: a store
    /// that contradicts it is rejected by the guard's `is_plain_number_bits` /
    /// the inline path's finite-exponent test, falls back to the boxed setter,
    /// and downgrades through [`layout_note_slot`] exactly as a post-install
    /// contradiction always has.
    FreshlyAllocated,
}

/// Rebuild a mask slice from the raw `(pointer, word count)` pair the FFI
/// signature carries.
///
/// #7578 keeps the construction path on the raw pair and materialises a slice
/// only where one is actually indexed. `slice::from_raw_parts` requires a
/// non-null aligned pointer, so every call used to open with two
/// null-to-`NonNull::dangling()` `csel` chains — twelve instructions to
/// normalise two arguments that the fast path below then never dereferences.
#[inline(always)]
unsafe fn mask_words<'a>(words: *const u64, word_count: u32) -> &'a [u64] {
    if words.is_null() || word_count == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(words, word_count as usize)
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn init_typed_shape_layout(
    user_ptr: usize,
    slot_count: usize,
    raw_f64_words: *const u64,
    raw_f64_word_count: u32,
    pointer_words: *const u64,
    pointer_word_count: u32,
    proof: TypedShapeProof,
) {
    // One `gc_type_layout_slot_kind`, not two. `layout_header_for_user`
    // computes the kind and accepts three of them; the line that used to follow
    // it recomputed the same kind — a second load through the 32-byte-strided
    // type table — to narrow those three to one. Requiring `ObjectFields`
    // directly is exactly equivalent, because `ObjectFields` is one of the
    // three `layout_header_for_user` admits (#7578).
    if user_ptr < GC_HEADER_SIZE + 0x1000 {
        return;
    }
    let header = header_from_user_ptr(user_ptr as *const u8);
    if gc_type_layout_slot_kind((*header).obj_type) != GcLayoutSlotKind::ObjectFields {
        return;
    }
    let obj_header = user_ptr as *const crate::object::ObjectHeader;
    let shape_descriptor = crate::object::shapes::object_shape_descriptor(obj_header);
    // #8113: 0, not a second (eager) descriptor probe.
    let object_slot_count = shape_descriptor
        .map(|descriptor| descriptor.live_inline_slot_count as usize)
        .unwrap_or(0);
    if object_slot_count != slot_count {
        layout_set_typed_unknown(header, user_ptr);
        return;
    }

    if slot_count != 0 && proof == TypedShapeProof::ValidateSlots {
        let raw_f64_words = mask_words(raw_f64_words, raw_f64_word_count);
        let pointer_words = mask_words(pointer_words, pointer_word_count);
        let fields = (obj_header as *const u8)
            .add(std::mem::size_of::<crate::object::ObjectHeader>())
            as *const u64;
        for i in 0..slot_count {
            let bits = *fields.add(i);
            if shape_install::words_contain_slot(raw_f64_words, i) {
                if !layout_raw_f64_bits(bits) {
                    layout_set_typed_unknown(header, user_ptr);
                    return;
                }
                continue;
            }
            if layout_pointer_bearing_bits(bits)
                && !shape_install::words_contain_slot(pointer_words, i)
            {
                layout_set_typed_unknown(header, user_ptr);
                return;
            }
        }
    }

    // #7510 item 1: everything above this line is re-derived per object and
    // stays that way — it is what makes the header declaration below true.
    // What the 20-millionth `{v, w}` literal does NOT need to re-derive is the
    // *map* answer: that its shape's canonical descriptor is already installed
    // and already equal to the one these mask globals describe. That is all
    // `shape_install::hit` asserts, and on a hit the construction reduces to
    // the two header bit-writes `shape_install_shared` would have performed —
    // no `TypedLayoutDescriptor` built, cloned and dropped, no `RefCell`
    // borrow, no hash of `keys`, no field-by-field descriptor comparison.
    //
    // The memo carries no state about the OBJECT. The one bit it carries about
    // the *masks* — whether the pointer mask is empty, which selects
    // `POINTER_FREE` vs `SIDE_MASK` — is a pure function of bytes the entry has
    // already matched by address and length, and those bytes are immutable
    // program constants. See `gc::shape_install` for the full staleness
    // argument.
    //
    // The receiver-kind guards are already discharged above (the low
    // addresses were rejected, `GcLayoutSlotKind::ObjectFields` was checked),
    // so read the field directly rather than re-walking the header.
    let keys = shape_descriptor
        .map(|descriptor| descriptor.keys as usize)
        .unwrap_or((*obj_header).keys_array as usize);
    let memo = if keys == 0 {
        None
    } else {
        shape_install::hit(
            keys,
            slot_count,
            raw_f64_words,
            raw_f64_word_count,
            pointer_words,
            pointer_word_count,
        )
    };
    if let Some(pointer_mask_empty) = memo {
        header_set_typed_layout_intact(header);
        if pointer_mask_empty {
            set_layout_state(header, GC_LAYOUT_POINTER_FREE);
        } else {
            set_layout_state(header, GC_LAYOUT_SIDE_MASK);
        }
        layout_forget_object(user_ptr);
        return;
    }

    install_typed_shape_layout_slow(
        user_ptr,
        header,
        keys,
        slot_count,
        raw_f64_words,
        raw_f64_word_count,
        pointer_words,
        pointer_word_count,
    );
}

/// The memo-MISS tail of [`init_typed_shape_layout`]: build the descriptor,
/// install it (shared by shape, else per object), record the memo.
///
/// #8122: kept OUT OF LINE on purpose. Everything above the memo probe runs on
/// every construction; this runs once per shape (plus downgrades). When LLVM's
/// LTO inliner chose to fold this tail — with `shape_install_shared` and
/// `shape_install::record` folded into it in turn — into the hot prologue, the
/// per-construction path became an 811-instruction function whose prologue,
/// spills and register pressure were paid on every memo hit: measured as a
/// reproducible +3.9% instructions on `pipeline` between two builds of the
/// SAME hot-path code, differing only in an unrelated module's size. `#[cold]`
/// + `#[inline(never)]` pins the shape the profile wants regardless of what
/// else moves in the crate.
#[cold]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
unsafe fn install_typed_shape_layout_slow(
    user_ptr: usize,
    header: *mut GcHeader,
    keys: usize,
    slot_count: usize,
    raw_f64_words: *const u64,
    raw_f64_word_count: u32,
    pointer_words: *const u64,
    pointer_word_count: u32,
) {
    let raw_f64_slice = mask_words(raw_f64_words, raw_f64_word_count);
    let pointer_slice = mask_words(pointer_words, pointer_word_count);

    // #7578: the mask-disjointness check moved down here, off the hit path.
    //
    // It is a pure function of the two mask globals, and a memo hit proves an
    // install already ran it over the *same* globals and passed — a shape whose
    // masks intersect is downgraded here and never reaches `record`, so no
    // intersecting tuple can be in the table to hit. Running it above the probe
    // charged every construction of every shape for a compile-time property of
    // the class. It stays ahead of every install, which is the only place its
    // answer is used.
    if shape_install::words_intersect(raw_f64_slice, pointer_slice, slot_count) {
        layout_set_typed_unknown(header, user_ptr);
        return;
    }

    let pointer_mask = LayoutSlotMask::from_words(pointer_slice);
    let descriptor = TypedLayoutDescriptor {
        slot_count,
        raw_f64_mask: LayoutSlotMask::from_words(raw_f64_slice),
        pointer_mask: pointer_mask.clone(),
    };
    // #6893: try the O(shapes) shared shape descriptor (keyed by the canonical
    // keys_array) before per-object storage. `shape_layout_keyed_enabled()`
    // gates this and therefore also gates the memo above: the memo is only
    // ever populated from a successful install here, so with the knob off the
    // table stays empty and every lookup misses.
    let keys = if shape_layout_keyed_enabled() {
        keys
    } else {
        0
    };
    if keys != 0 && shape_install_shared(keys, header, &descriptor) {
        // The common path for every object literal: the shape already owns a
        // canonical descriptor, so this object needs no per-object record at
        // all. `layout_forget_object` skips the hash entirely when the maps
        // are empty, which on a monomorphic workload they are.
        //
        // `pointer_mask.is_empty()` is what the hit path above will replay from
        // the memo; `words_are_empty` is pinned equal to it by
        // `shape_install::tests::mask_word_helpers_agree_with_layout_slot_mask`.
        shape_install::record(
            keys,
            slot_count,
            raw_f64_words,
            raw_f64_word_count,
            pointer_words,
            pointer_word_count,
            pointer_mask.is_empty(),
        );
        layout_forget_object(user_ptr);
        return;
    }
    typed_layouts_insert(user_ptr, descriptor);
    header_set_typed_layout_intact(header);
    if pointer_mask.is_empty() {
        set_layout_state(header, GC_LAYOUT_POINTER_FREE);
        slot_masks_remove(user_ptr);
    } else {
        set_layout_state(header, GC_LAYOUT_SIDE_MASK);
        slot_masks_insert(user_ptr, pointer_mask);
    }
}

#[inline]
fn typed_shape_layout_entry(
    obj: u64,
    slot_count: u32,
    raw_f64_mask_words: *const u64,
    raw_f64_mask_word_count: u32,
    pointer_mask_words: *const u64,
    pointer_mask_word_count: u32,
    proof: TypedShapeProof,
) {
    let user_ptr = strip_nanbox_user_ptr(obj);
    let slot_count = slot_count as usize;
    if user_ptr == 0 || slot_count > 16_000_000 {
        return;
    }
    unsafe {
        init_typed_shape_layout(
            user_ptr,
            slot_count,
            raw_f64_mask_words,
            raw_f64_mask_word_count,
            pointer_mask_words,
            pointer_mask_word_count,
            proof,
        );
    }
}

/// Register a constructed instance's canonical layout **after** its fields hold
/// their final values. Validates every slot before promoting.
#[no_mangle]
pub extern "C" fn js_gc_init_typed_shape_layout(
    obj: u64,
    slot_count: u32,
    raw_f64_mask_words: *const u64,
    raw_f64_mask_word_count: u32,
    pointer_mask_words: *const u64,
    pointer_mask_word_count: u32,
) {
    typed_shape_layout_entry(
        obj,
        slot_count,
        raw_f64_mask_words,
        raw_f64_mask_word_count,
        pointer_mask_words,
        pointer_mask_word_count,
        TypedShapeProof::ValidateSlots,
    );
}

/// #7510: register a **freshly allocated** instance's canonical layout, before
/// its constructor runs, so the constructor's own field stores can pass the
/// `GC_OBJ_TYPED_LAYOUT_INTACT` guard.
///
/// `js_gc_init_typed_shape_layout` cannot be moved earlier: it validates that
/// each raw-f64 slot already holds a plain double, and a fresh slot holds
/// `TAG_UNDEFINED`, so an early call downgrades every instance it touches. That
/// is why a declared-`number` class field was *slower* than the equivalent
/// object literal (#7512) — the descriptor arrived after the only stores that
/// wanted it, so every one fell back to `js_put_value_set`.
///
/// This form carries the proof on the codegen side instead; see
/// [`TypedShapeProof::FreshlyAllocated`] for what it rests on and why the
/// collector's half is unconditional. **Callers must invoke it only on an
/// instance whose slots are still the allocator's fill** — the whole contract
/// is "nothing has been written yet", and it is not checkable from here.
#[no_mangle]
pub extern "C" fn js_gc_declare_typed_shape_layout(
    obj: u64,
    slot_count: u32,
    raw_f64_mask_words: *const u64,
    raw_f64_mask_word_count: u32,
    pointer_mask_words: *const u64,
    pointer_mask_word_count: u32,
) {
    typed_shape_layout_entry(
        obj,
        slot_count,
        raw_f64_mask_words,
        raw_f64_mask_word_count,
        pointer_mask_words,
        pointer_mask_word_count,
        TypedShapeProof::FreshlyAllocated,
    );
}
