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

/// Process-global typed class layouts installed by codegen at module init.
///
/// Ordinary `SHAPE_LAYOUTS` entries are agent-local because they are learned
/// from objects at runtime. These entries describe immutable code-image masks
/// and use a dedicated ShapeId, so the descriptor is valid in every worker and
/// can be copied into that worker's hot table on first use.
#[derive(Clone, Hash, PartialEq, Eq)]
struct RegisteredTypedShapeKey {
    class_id: u32,
    slot_count: u32,
    raw_f64_words: Vec<u64>,
    pointer_words: Vec<u64>,
}

#[derive(Default)]
struct RegisteredTypedShapes {
    /// `FastKeyHasher`, NOT `PtrHasher`: [`RegisteredTypedShapeKey`] is a
    /// COMPOSITE key (two `u32`s plus two `Vec<u64>` mask word lists), and
    /// `PtrHasher`'s `write_*` OVERWRITE the accumulator, which would collapse
    /// the key to its last field. `FastKeyHasher` folds every field.
    ///
    /// Neither half is external input — `class_id` is codegen-minted and the
    /// mask words are derived from the compiled class layout — so SipHash's
    /// DoS resistance buys nothing. The derived `Hash` feeds each mask word
    /// through `write_u64`, which SipHash charges per byte; the word-at-a-time
    /// folds added in #9147 make that one multiply per word.
    ids_by_layout: crate::fast_hash::FastKeyHashMap<RegisteredTypedShapeKey, u32>,
    /// Bare `u32` ShapeId key -> `PtrHasher` (single multiply + avalanche).
    layouts_by_id: crate::fast_hash::PtrHashMap<u32, TypedLayoutDescriptor>,
    /// `(class id, slot count)` -> the first typed ShapeId registered for it.
    /// Read when an importing module registers its compiled ShapeId slots.
    typed_by_class: std::collections::HashMap<(u32, u32), u32>,
    /// Importing modules' compiled ShapeId slots still waiting for their
    /// class's typed ShapeId. See [`js_register_imported_class_shape_slot`].
    pending_imported: std::collections::HashMap<(u32, u32), Vec<ImportedShapeSlot>>,
}

/// The ADDRESSES of one importing module's compiled per-class globals: its
/// keys-array global, its `u32` ShapeId global and, when it composes one, its
/// `<2 x i64>` inline-`new` header image. All three live in the executable's
/// data section for the life of the process (codegen registers them only for
/// images that are never unloaded), and none of them is a heap pointer: the
/// keys global's CONTENTS are one, rewritten by the collector because codegen
/// registers that global as a root, and this is only ever read at the moment
/// of a rewrite.
#[derive(Clone, Copy)]
struct ImportedShapeSlot {
    keys_slot: usize,
    shape_slot: usize,
    image_slot: usize,
    /// The importing module's `@perry_class_guard_shape_*` twin of
    /// `shape_slot`. It must follow the same rewrite, or an imported class's
    /// inline guard would compare against a stale expectation and miss for the
    /// life of the process. 0 when the stub predates the guard global.
    guard_slot: usize,
}

static REGISTERED_TYPED_SHAPES: std::sync::LazyLock<std::sync::Mutex<RegisteredTypedShapes>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(RegisteredTypedShapes::default()));

fn registered_typed_shapes() -> std::sync::MutexGuard<'static, RegisteredTypedShapes> {
    REGISTERED_TYPED_SHAPES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Copy a module-init descriptor for `shape_id`, if this is one of #8405's
/// dedicated typed class shapes. The caller installs the copy in its
/// agent-local hot table, making the global mutex a once-per-shape/thread cold
/// path rather than a trace/store cost.
pub(super) fn registered_typed_shape_layout(shape_id: u32) -> Option<TypedLayoutDescriptor> {
    registered_typed_shapes()
        .layouts_by_id
        .get(&shape_id)
        .cloned()
}

/// Mint (or reuse) a ShapeId whose identity includes the exact typed layout.
/// Called once per eligible class at module initialization, before its header
/// image is published. Every allocation can therefore stamp
/// `SIDE_MASK | TYPED_LAYOUT_INTACT` without a per-object runtime call.
#[no_mangle]
pub extern "C" fn js_gc_typed_shape_id_for_keys(
    class_id: u32,
    keys: u64,
    slot_count: u32,
    raw_f64_words: *const u64,
    raw_f64_word_count: u32,
    pointer_words: *const u64,
    pointer_word_count: u32,
) -> u32 {
    if class_id == 0 || keys == 0 || slot_count >= 16_000_000 {
        eprintln!("Perry internal error: invalid pre-registered typed shape");
        std::process::abort();
    }
    let (raw_f64_slice, pointer_slice) = unsafe {
        (
            mask_words(raw_f64_words, raw_f64_word_count),
            mask_words(pointer_words, pointer_word_count),
        )
    };
    if pointer_slice.is_empty()
        || shape_install::words_intersect(raw_f64_slice, pointer_slice, slot_count as usize)
    {
        eprintln!("Perry internal error: invalid pre-registered typed shape masks");
        std::process::abort();
    }
    let key = RegisteredTypedShapeKey {
        class_id,
        slot_count,
        raw_f64_words: raw_f64_slice.to_vec(),
        pointer_words: pointer_slice.to_vec(),
    };
    let descriptor = TypedLayoutDescriptor {
        slot_count: slot_count as usize,
        raw_f64_mask: LayoutSlotMask::from_words(raw_f64_slice),
        pointer_mask: LayoutSlotMask::from_words(pointer_slice),
    };
    let mut registered = registered_typed_shapes();
    if let Some(&shape_id) = registered.ids_by_layout.get(&key) {
        if !crate::object::shapes::install_registered_typed_shape_id(
            shape_id,
            keys as usize as *const crate::array::ArrayHeader,
            slot_count,
            class_id,
        ) {
            eprintln!("Perry internal error: typed ShapeId structural mismatch");
            std::process::abort();
        }
        publish_to_imported_slots(&mut registered, class_id, slot_count, shape_id);
        return shape_id;
    }
    let shape_id = crate::object::shapes::mint_registered_typed_shape_id(
        keys as usize as *const crate::array::ArrayHeader,
        slot_count,
        class_id,
    );
    registered.ids_by_layout.insert(key, shape_id);
    registered.layouts_by_id.insert(shape_id, descriptor);
    publish_to_imported_slots(&mut registered, class_id, slot_count, shape_id);
    shape_id
}

/// Point every importing module's compiled ShapeId slot for `class_id` at the
/// defining module's typed ShapeId, whatever order the modules initialized in.
///
/// A module mints the ShapeId of every class it allocates — imported stubs
/// included — in its string-pool initializer, which runs at the start of that
/// module's init. Only the DEFINING module can mint #8405's typed id (only it
/// has the constructor that proves the layout); a consumer mints the ordinary
/// structural id, and `js_object_shape_id_for_keys` returns the typed id only
/// if the defining module has already registered it. Whenever a consumer's
/// string pool runs first — the entry module, whose pool `main` runs before any
/// dependency init; a module in an import cycle; a deferred module's cycle —
/// the class carried two identities and every instance the consumer allocated
/// missed the defining module's exact field-store guards (n.next = m: 2,786
/// instructions per store against 224).
///
/// Codegen therefore registers each imported stub's slots
/// ([`js_register_imported_class_shape_slot`]), and this rewrites them to the
/// typed id: immediately if it already exists, otherwise when it is minted.
///
/// Soundness: both ids are valid descriptors of the same keys array and slot
/// count, so every object keeps working whichever id it was born with.
/// `install_registered_typed_shape_id` checks that structural match against the
/// consumer's own keys global before any slot is written. The consumer's
/// header image carries no typed-layout claim for an imported class, so an
/// instance stamped with the typed id still validates its slots in
/// `js_gc_init_typed_shape_layout` before it claims `TYPED_LAYOUT_INTACT`.
/// Instances born before the rewrite keep the ordinary id, which only makes
/// their exact guards miss onto the runtime fallback. The ShapeId slot is one
/// `u32` store and the image rewrite one `u64` store of its second word, so a
/// reader never observes a torn word; a reader that pairs the new id with the
/// old image (or the reverse) builds an object whose guard misses — slow, not
/// wrong. The registry lock serializes registration against publication.
fn publish_to_imported_slots(
    registered: &mut RegisteredTypedShapes,
    class_id: u32,
    slot_count: u32,
    shape_id: u32,
) {
    registered
        .typed_by_class
        .entry((class_id, slot_count))
        .or_insert(shape_id);
    if let Some(slots) = registered.pending_imported.remove(&(class_id, slot_count)) {
        for slot in slots {
            unsafe { rewrite_imported_shape_slot(slot, class_id, slot_count, shape_id) };
        }
    }
}

/// # Safety
/// `slot` must hold the addresses codegen registered: a live `u64` keys global,
/// a `u32` ShapeId global and a null or `<2 x i64>` header image global.
unsafe fn rewrite_imported_shape_slot(
    slot: ImportedShapeSlot,
    class_id: u32,
    slot_count: u32,
    shape_id: u32,
) {
    let keys = std::ptr::read(slot.keys_slot as *const u64);
    if keys == 0
        || !crate::object::shapes::install_registered_typed_shape_id(
            shape_id,
            keys as usize as *const crate::array::ArrayHeader,
            slot_count,
            class_id,
        )
    {
        return;
    }
    std::ptr::write(slot.shape_slot as *mut u32, shape_id);
    if slot.guard_slot != 0 {
        // The guard expectation follows the ShapeId — unless the inline path
        // has already been disabled, in which case it must stay poisoned. A
        // stub registered before the flip and rewritten after it would
        // otherwise reopen a fast path the process has closed.
        let value = if crate::object::class_field_inline_guard_enabled() {
            shape_id
        } else {
            crate::object::CLASS_GUARD_SHAPE_POISON
        };
        std::ptr::write(slot.guard_slot as *mut u32, value);
    }
    if slot.image_slot != 0 {
        let word = (slot.image_slot as *mut u64).add(1);
        let class_id_bits = std::ptr::read(word) & 0xFFFF_FFFF;
        std::ptr::write(word, ((shape_id as u64) << 32) | class_id_bits);
    }
}

/// Register an importing module's compiled ShapeId slots for `class_id` so
/// they follow the defining module's typed ShapeId (see
/// [`publish_to_imported_slots`]). Called once per imported class stub from
/// the module's string-pool initializer, after it stored its own id and image.
///
/// `keys_slot`, `shape_slot` and `image_slot` are addresses of compiled
/// globals, never heap pointers; codegen emits this call only for images that
/// are never unloaded, because the registry keeps the addresses.
#[no_mangle]
pub extern "C" fn js_register_imported_class_shape_slot(
    class_id: u32,
    slot_count: u32,
    keys_slot: *const u64,
    shape_slot: *mut u32,
    image_slot: *mut u64,
    guard_slot: *mut u32,
) {
    if class_id == 0 || keys_slot.is_null() || shape_slot.is_null() || slot_count >= 16_000_000 {
        return;
    }
    let slot = ImportedShapeSlot {
        keys_slot: keys_slot as usize,
        shape_slot: shape_slot as usize,
        image_slot: image_slot as usize,
        guard_slot: guard_slot as usize,
    };
    // Also enrol the guard expectation for process-wide poisoning, so a LATER
    // `disable_class_field_inline_guard` reaches an imported class's slot too.
    if !guard_slot.is_null() {
        // SAFETY: a compiled `@perry_class_guard_shape_*` global, static and
        // writable for the life of the image (the caller's contract above).
        unsafe { crate::object::js_register_class_guard_shape(guard_slot) };
    }
    let mut registered = registered_typed_shapes();
    match registered
        .typed_by_class
        .get(&(class_id, slot_count))
        .copied()
    {
        Some(shape_id) => unsafe {
            rewrite_imported_shape_slot(slot, class_id, slot_count, shape_id)
        },
        None => registered
            .pending_imported
            .entry((class_id, slot_count))
            .or_default()
            .push(slot),
    }
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_REGISTER_IMPORTED_CLASS_SHAPE_SLOT: extern "C" fn(
    u32,
    u32,
    *const u64,
    *mut u32,
    *mut u64,
    *mut u32,
) = js_register_imported_class_shape_slot;

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
    // Constructor return override may replace a freshly allocated instance
    // with an exotic object represented by a registry handle (notably Proxy).
    // Codegen still offers the completed receiver to this post-construction
    // layout hook. Reject anything that is not an actual GC allocation before
    // deriving and dereferencing its preceding header.
    if crate::value::addr_class::try_read_gc_header(user_ptr).is_none() {
        return;
    }
    let header = header_from_user_ptr(user_ptr as *const u8);
    if gc_type_layout_slot_kind((*header).obj_type) != GcLayoutSlotKind::ObjectFields {
        return;
    }
    let obj_header = user_ptr as *const crate::object::ObjectHeader;
    let mut shape_id = crate::object::shapes::object_shape_stamp(obj_header);

    // #8289: ShapeIds are immutable, process-unique names for the exact live
    // slot bound as well as the ordered keys. Once this tuple has passed the
    // authoritative descriptor check, its memo entry can replay that proof
    // for every sibling without hashing the ShapeTable again. A miss still
    // resolves the descriptor and performs the same downgrade as before.
    let memo = if shape_id == 0 {
        None
    } else {
        shape_install::hit(
            shape_id,
            slot_count,
            raw_f64_words,
            raw_f64_word_count,
            pointer_words,
            pointer_word_count,
        )
    };
    if memo.is_none() {
        #[cfg(test)]
        shape_install::note_descriptor_probe();
        let shape_descriptor = crate::object::shapes::object_shape_descriptor(obj_header);
        let object_slot_count = shape_descriptor
            .map(|descriptor| descriptor.live_inline_slot_count as usize)
            .unwrap_or(0);
        if object_slot_count != slot_count {
            layout_set_typed_unknown(header, user_ptr);
            return;
        }
        // Keyless `js_object_alloc` objects deliberately use per-object
        // descriptors: without property names there is no shared semantic
        // shape to own one. Preserve that contract even though these objects
        // are now birth-stamped for their authoritative live-slot bound.
        if shape_descriptor.is_none_or(|descriptor| descriptor.keys == 0) {
            shape_id = 0;
        }
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
    if let Some(pointer_mask_empty) = memo {
        shape_install::note_hit();
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
        shape_id,
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
    shape_id: u32,
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
    // #6893: try the O(shapes) shared shape descriptor (keyed by immutable
    // runtime ShapeId) before per-object storage. `shape_layout_keyed_enabled()`
    // gates this and therefore also gates the memo above: the memo is only
    // ever populated from a successful install here, so with the knob off the
    // table stays empty and every lookup misses.
    let shape_id = if shape_layout_keyed_enabled() {
        shape_id
    } else {
        0
    };
    if shape_id != 0 && shape_install_shared(shape_id, header, &descriptor) {
        // The common path for every object literal: the shape already owns a
        // canonical descriptor, so this object needs no per-object record at
        // all. `layout_forget_object` skips the hash entirely when the maps
        // are empty, which on a monomorphic workload they are.
        //
        // `pointer_mask.is_empty()` is what the hit path above will replay from
        // the memo; `words_are_empty` is pinned equal to it by
        // `shape_install::tests::mask_word_helpers_agree_with_layout_slot_mask`.
        shape_install::record(
            shape_id,
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

#[cfg(test)]
mod imported_shape_slot_tests {
    use super::*;

    fn keys_for(class_id: u32, packed: &[u8]) -> u64 {
        crate::object::js_build_class_keys_array(class_id, 2, packed.as_ptr(), packed.len() as u32)
            as usize as u64
    }

    struct Slots {
        keys: Box<u64>,
        shape: Box<u32>,
        image: Box<[u64; 2]>,
        /// The poisonable guard expectation twinned with `shape`. It must
        /// follow every rewrite `shape` gets, or an imported class's inline
        /// field guard compares against a stale value for the whole process.
        guard: Box<u32>,
    }

    fn slots(class_id: u32, packed: &[u8]) -> Slots {
        let keys = keys_for(class_id, packed);
        let ordinary = crate::object::shapes::js_object_shape_id_for_keys(keys, 2);
        Slots {
            keys: Box::new(keys),
            shape: Box::new(ordinary),
            image: Box::new([0x1234_5678, ((ordinary as u64) << 32) | class_id as u64]),
            guard: Box::new(ordinary),
        }
    }

    fn register(class_id: u32, s: &mut Slots) {
        js_register_imported_class_shape_slot(
            class_id,
            2,
            &*s.keys as *const u64,
            &mut *s.shape as *mut u32,
            s.image.as_mut_ptr(),
            &mut *s.guard as *mut u32,
        );
    }

    fn mint(class_id: u32, keys: u64) -> u32 {
        let raw_mask = [0b10u64];
        let pointer_mask = [0b01u64];
        js_gc_typed_shape_id_for_keys(
            class_id,
            keys,
            2,
            raw_mask.as_ptr(),
            1,
            pointer_mask.as_ptr(),
            1,
        )
    }

    fn assert_published(class_id: u32, s: &Slots, typed: u32) {
        assert_eq!(*s.shape, typed, "the ShapeId slot follows the typed id");
        assert_eq!(
            s.image[0], 0x1234_5678,
            "the consumer's packed word is untouched"
        );
        assert_eq!(s.image[1], ((typed as u64) << 32) | class_id as u64);
        // Both rewrite paths must carry the guard expectation with them. If
        // this drifts, an imported class's inline field guard compares a live
        // object against the stub's ORDINARY id forever — it never goes wrong,
        // it just silently never hits, which no correctness test would catch.
        assert_eq!(
            *s.guard, typed,
            "the guard expectation follows the ShapeId slot"
        );
    }

    /// The consumer registers first (its string pool ran before the defining
    /// module's init): the slots are rewritten when the typed id is minted.
    #[test]
    fn slots_registered_before_the_typed_id_are_rewritten_at_mint() {
        let class_id = 0x0B1_1001;
        let mut s = slots(class_id, b"next\0value\0");
        let ordinary = *s.shape;
        register(class_id, &mut s);
        assert_eq!(*s.shape, ordinary, "nothing typed exists yet");
        let typed = mint(class_id, *s.keys);
        assert_ne!(typed, ordinary);
        assert_published(class_id, &s, typed);
    }

    /// The defining module initialized first: registration rewrites at once.
    #[test]
    fn slots_registered_after_the_typed_id_are_rewritten_immediately() {
        let class_id = 0x0B1_1002;
        let mut s = slots(class_id, b"next\0value\0");
        let typed = mint(class_id, *s.keys);
        register(class_id, &mut s);
        assert_published(class_id, &s, typed);
    }

    /// Disabling the inline path poisons a registered expectation, and a
    /// LATER rewrite must not resurrect it.
    #[test]
    fn a_disabled_inline_path_keeps_imported_expectations_poisoned() {
        let class_id = 0x0B1_1005;
        let mut s = slots(class_id, b"next\0value\0");
        register(class_id, &mut s);
        crate::object::disable_class_field_inline_guard();
        assert_eq!(
            *s.guard,
            crate::object::CLASS_GUARD_SHAPE_POISON,
            "disabling must poison an already-registered expectation"
        );
        let typed = mint(class_id, *s.keys);
        assert_eq!(*s.shape, typed, "the ShapeId slot still follows the mint");
        assert_eq!(
            *s.guard,
            crate::object::CLASS_GUARD_SHAPE_POISON,
            "a rewrite after the disable must NOT reopen the fast path"
        );
        crate::object::test_reset_class_field_inline_guard();
    }

    /// A slot whose keys global does not hold the typed id's keys array, or
    /// whose slot count differs, is never rewritten.
    #[test]
    fn mismatched_slots_keep_their_ordinary_id() {
        let class_id = 0x0B1_1003;
        let other_class = 0x0B1_1004;
        // A different class id with equal keys now shares the same canonical
        // array. Use different names, with the same slot count, to isolate
        // the keys mismatch from the independent count mismatch below.
        let mut foreign = slots(other_class, b"other_next\0other_value\0");
        let foreign_ordinary = *foreign.shape;
        js_register_imported_class_shape_slot(
            class_id,
            2,
            &*foreign.keys as *const u64,
            &mut *foreign.shape as *mut u32,
            foreign.image.as_mut_ptr(),
            &mut *foreign.guard as *mut u32,
        );
        let mut narrow = slots(class_id, b"next\0value\0");
        assert_ne!(
            *foreign.keys, *narrow.keys,
            "fixture requires different canonical keys arrays"
        );
        let narrow_ordinary = *narrow.shape;
        js_register_imported_class_shape_slot(
            class_id,
            1,
            &*narrow.keys as *const u64,
            &mut *narrow.shape as *mut u32,
            std::ptr::null_mut(),
            &mut *narrow.guard as *mut u32,
        );
        let _typed = mint(class_id, *narrow.keys);
        assert_eq!(
            *foreign.shape, foreign_ordinary,
            "another class's keys array"
        );
        assert_eq!(*narrow.shape, narrow_ordinary, "a different slot count");
    }
}
