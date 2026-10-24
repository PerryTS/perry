#[inline]
pub(crate) unsafe fn refresh_array_numeric_layout(arr: *mut ArrayHeader) {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return;
    }
    refresh_array_numeric_layout_resolved(arr);
}

/// [`refresh_array_numeric_layout`] for a head the caller already resolved.
#[inline]
pub(crate) unsafe fn refresh_array_numeric_layout_resolved(arr: *mut ArrayHeader) {
    // Canonicalization already validates every slot. Avoid a separate full
    // validation pass. A failing mixed payload may have had numeric boxes in
    // its prefix canonicalized, which preserves their JS values; it must
    // still lose both numeric-layout claims (including on a hole).
    if !rebuild_array_numeric_raw_f64(arr) {
        clear_array_numeric_layout(arr);
    }
}

#[no_mangle]
pub extern "C" fn js_array_mark_numeric_f64_layout(arr: *mut ArrayHeader) -> i32 {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return 0;
    }
    unsafe {
        if !array_slots_are_numeric(arr) {
            clear_array_numeric_layout(arr);
            return 0;
        }
        rebuild_array_numeric_raw_f64(arr);
    }
    1
}

#[no_mangle]
pub extern "C" fn js_array_clear_numeric_layout(arr: *mut ArrayHeader) {
    let arr = clean_arr_ptr_mut(arr);
    unsafe {
        clear_array_numeric_layout(arr);
    }
}

/// The first non-Number write into a raw-f64 array whose header the caller
/// just probed: clear both raw bits in place. Out of line so the common exits
/// of the note (Number, or bits already clear) keep a frameless prologue.
///
/// # Safety
/// `arr` is a live, non-forwarded `GC_TYPE_ARRAY` head (the caller's probe).
#[inline(never)]
unsafe fn demote_probed_raw_f64_array(arr: *mut ArrayHeader) {
    let header = (arr as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
    (*header)._reserved &=
        !(crate::gc::GC_ARRAY_RAW_F64_LAYOUT | crate::gc::GC_ARRAY_RAW_F64_HOLES);
    crate::typed_feedback::invalidate_representation_change(arr as usize);
}

#[no_mangle]
pub extern "C" fn js_array_note_numeric_write(arr: *mut ArrayHeader, value_bits: u64) {
    // A Number never downgrades the raw-f64 layout: nothing to clear, so do
    // not pay the receiver resolver for it.
    if value_bits_are_numeric(value_bits) {
        return;
    }
    // Exact, non-forwarded ordinary Array: the magnitude-checked live header
    // probe (the same discipline as the generated guards) answers without the
    // tracked-allocation resolver. Raw bits already clear: the note is a
    // no-op. Raw bit set: this is the first non-Number write, and clearing the
    // two bits in the header is the whole demotion. Forwarding stubs and every
    // other brand keep the complete resolver below.
    let raw_bits = crate::gc::GC_ARRAY_RAW_F64_LAYOUT | crate::gc::GC_ARRAY_RAW_F64_HOLES;
    let probed = unsafe { crate::value::addr_class::try_read_gc_header(arr as usize) }
        .filter(|header| {
            header.obj_type == crate::gc::GC_TYPE_ARRAY
                && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
        })
        .map(|header| header._reserved & raw_bits != 0);
    match probed {
        Some(false) => return,
        Some(true) => {
            // SAFETY: the probe just validated this live, non-forwarded
            // array header; this is the same header store the
            // resolver path performs once it has returned this head.
            unsafe { demote_probed_raw_f64_array(arr) };
            return;
        }
        None => {}
    }
    let arr = clean_arr_ptr_mut(arr);
    unsafe {
        note_array_numeric_write(arr, value_bits);
    }
}

/// The whole F64-kind cold arm of a generated element store
/// (perry-codegen `index_set_guarded::emit_array_store_kind_value`), one call
/// where the arm made two and re-read the kind between them.
///
/// The note comes first, exactly as before: a non-Number clears the array's
/// raw-f64 kind before its bits can land in a slot a raw-f64 reader trusts.
/// Then the answer is what the store must write: the value as its raw double
/// while the kind still holds (a Number: an `INT32` box), the value itself once
/// the note has cleared it. `arr` is the store's validated array handle; the
/// kind is the same `_reserved` half-word the emitted fast test reads.
#[no_mangle]
pub extern "C" fn js_array_note_numeric_write_value(arr: *mut ArrayHeader, value: f64) -> f64 {
    js_array_note_numeric_write(arr, value.to_bits());
    let raw_bits = crate::gc::GC_ARRAY_RAW_F64_LAYOUT | crate::gc::GC_ARRAY_RAW_F64_HOLES;
    let reserved = unsafe {
        (*((arr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader))
            ._reserved
    };
    if reserved & raw_bits != 0 {
        js_array_numeric_value_to_raw_f64(value)
    } else {
        value
    }
}

/// #7469 — declare ONCE, at allocation, that every element this array will
/// hold is a heap pointer, so the per-store pointer-mask bookkeeping
/// (`layout_note_slot` and the header payload kind) is not
/// needed for the stores codegen has proven pointer-valued.
///
/// Emitted by codegen at the `[]` literal that binds an array local whose every
/// store it can prove pointer-by-construction; see
/// `perry-codegen/src/collectors/all_pointer_arrays.rs` for the proof and
/// `expr/array_push.rs` for the header test that re-validates the declaration
/// at every elided store.
///
/// Two things happen here, and both are load-bearing:
///
/// 1. **The raw-f64 numeric layout is cleared.** `js_array_alloc` publishes
///    every fresh array as `RawF64` + `POINTER_FREE` — the numeric fast paths
///    read such an array's slots back as raw doubles, which for a pointer
///    payload is a reinterpretation of a heap address as a number. The
///    all-pointer declaration and the raw-f64 flag are mutually exclusive
///    claims about the same bytes, and the codegen-side header test refuses the
///    elided store unless BOTH raw-f64 bits are clear, so this clear is what
///    keeps that test satisfiable.
/// 2. **`GC_LAYOUT_SIDE_MASK | GC_LAYOUT_ALL_POINTERS` replaces
///    `GC_LAYOUT_POINTER_FREE`.** For the collector that swaps "skip the whole
///    payload" for "visit every slot in `0..length`" — the same set of slots
///    `GC_LAYOUT_UNKNOWN` visits, so the declaration is conservative in the
///    only direction that matters: a wrong element type costs a rejected slot
///    read (`mark_field_into_worklist` re-validates every word), never a
///    stranded live child.
///
/// On a **non-empty** array the claim is not vacuous, so it is discharged
/// rather than assumed: every slot in `0..length` must be pointer-bearing by
/// `gc::layout_pointer_bearing_bits`, the same predicate the layout mask
/// builder and `GC_LAYOUT_UNKNOWN`'s per-slot re-validation use. The walk is
/// O(literal size), runs once at the binding, and does not have to trust the
/// caller's static proof.
///
/// #8102 is why that path exists. `emit_all_pointer_array_declaration` is
/// emitted from the `Stmt::Let` tail, i.e. *after* an array literal's element
/// stores have already installed a per-slot side mask. Refusing every
/// non-empty array therefore made the declaration a **silent no-op** for
/// `const a: C[] = [x, y]`, so every later `a.push(…)` failed the codegen
/// header test and paid the per-store layout note #7469 exists to delete —
/// measured at +33.9% instructions on a 4M-push loop, against the
/// byte-for-byte equivalent array built empty and pushed into.
/// `collectors/all_pointer_arrays.rs` already admits such a literal (see
/// `literal_of_object_elements_is_admitted`), so the proof was being issued and
/// then discarded.
///
/// A refusal is still silent and safe — the header keeps whatever layout it
/// had, and the codegen header test then declines the elided store and routes
/// the push through `js_array_push_f64`, which notes every slot as it always
/// did.
#[no_mangle]
pub extern "C" fn js_array_declare_all_pointer_elements(arr: *mut ArrayHeader) {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return;
    }
    unsafe {
        let length = (*arr).length as usize;
        let slots = if length == 0 {
            std::ptr::null()
        } else {
            array_elements_ptr(arr) as *const u64
        };
        // Clear the raw-f64 claim FIRST only when the declaration will stick:
        // a refused declaration must leave the header exactly as it found it.
        if !crate::gc::layout_all_pointer_slots_would_hold(slots, length) {
            return;
        }
        clear_array_numeric_layout(arr);
        crate::gc::layout_init_all_pointer_slots(arr as *mut u8);
    }
}

#[no_mangle]
pub extern "C" fn js_array_is_numeric_f64_layout(arr: *const ArrayHeader) -> i32 {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return 0;
    }
    unsafe { js_array_is_numeric_f64_layout_resolved(arr) }
}

/// [`js_array_is_numeric_f64_layout`] for a caller holding a resolved head.
///
/// The typed-feedback push guard reaches this with a pointer it has already
/// normalized, header-checked and proved non-forwarded, so the entry's own
/// `clean_arr_ptr` — and the second one `array_numeric_layout` used to perform
/// inside it — were both re-deriving that proof once per push.
///
/// # Safety
/// `arr` is a non-null resolved head with no intervening allocation or
/// safepoint.
pub(crate) unsafe fn js_array_is_numeric_f64_layout_resolved(arr: *const ArrayHeader) -> i32 {
    unsafe {
        if array_numeric_layout_resolved(arr) == Some(NumericArrayLayout::RawF64) {
            return 1;
        }
        // #6011 follow-up: a holes-flagged array (`new Array(n)` mid-fill)
        // cannot be DENSE raw-f64, and answering that per store via the
        // verify walk below made a sequential fill loop O(N²) — the walk
        // reads the already-filled prefix on every store before hitting the
        // next hole (the fmr benchmark hang). While the LAST slot is still a
        // hole the array is provably not dense: answer 0 in O(1). Once the
        // last slot is written (a completed sequential fill), fall through so
        // the single walk below can upgrade the array to the dense flag.
        if array_has_raw_f64_holes_flag(arr) {
            let length = (*arr).length as usize;
            if length > 0 && array_slot_bits(arr, length - 1) == crate::value::TAG_HOLE {
                return 0;
            }
        }
        // #6011: single combined verify+rewrite pass. The old shape
        // (`array_slots_are_numeric` scan, THEN `rebuild_array_numeric_raw_
        // f64` rewrite) walked every slot twice on the first numeric-layout
        // probe of a large array — 2×100k slot reads per call for an
        // EMA-style `new Array(100_000)` filled by a loop. The rebuild
        // already fails cleanly on the first non-numeric slot (clearing the
        // layout flag); slots converted before such a failure were genuinely
        // numeric, and INT32-box → raw-f64 canonicalization is value-
        // preserving for every reader, so stopping mid-way is unobservable.
        if rebuild_array_numeric_raw_f64(arr as *mut ArrayHeader) {
            return 1;
        }
    }
    0
}

// These raw numeric-array helpers are called from generated code, so release/LTO
// builds may otherwise internalize and strip the `#[no_mangle]` exports.
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_ARRAY_NUMERIC_VALUE_TO_RAW_F64: extern "C" fn(f64) -> f64 =
    js_array_numeric_value_to_raw_f64;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_ARRAY_MARK_NUMERIC_F64_LAYOUT: extern "C" fn(*mut ArrayHeader) -> i32 =
    js_array_mark_numeric_f64_layout;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_ARRAY_CLEAR_NUMERIC_LAYOUT: extern "C" fn(*mut ArrayHeader) =
    js_array_clear_numeric_layout;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_ARRAY_NOTE_NUMERIC_WRITE: extern "C" fn(*mut ArrayHeader, u64) =
    js_array_note_numeric_write;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_ARRAY_NOTE_NUMERIC_WRITE_VALUE: extern "C" fn(*mut ArrayHeader, f64) -> f64 =
    js_array_note_numeric_write_value;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_ARRAY_DECLARE_ALL_POINTER_ELEMENTS: extern "C" fn(*mut ArrayHeader) =
    js_array_declare_all_pointer_elements;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_ARRAY_IS_NUMERIC_F64_LAYOUT: extern "C" fn(*const ArrayHeader) -> i32 =
    js_array_is_numeric_f64_layout;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_ARRAY_REFRESH_LOCAL_HEAD: extern "C" fn(f64) -> f64 = js_array_refresh_local_head;

/// The payload byte size of an array allocation with `slots` physical slots
/// (capacity plus any named-property reserve).
///
/// Every array allocation sizes its block here, so this is where
/// [`super::alloc::ARRAY_MAX_CAPACITY`] is enforced for the physical total:
/// a larger request raises `RangeError: Invalid array length` and reserves
/// nothing.
#[inline]
pub(crate) fn array_byte_size(slots: usize) -> usize {
    if slots > super::alloc::ARRAY_MAX_CAPACITY as usize {
        super::alloc::array_length_range_error()
    }
    std::mem::size_of::<ArrayHeader>() + slots * std::mem::size_of::<f64>()
}

#[inline]
pub(super) fn checked_array_allocation_size(capacity: usize) -> Option<usize> {
    capacity
        .checked_mul(std::mem::size_of::<f64>())
        .and_then(|elements| std::mem::size_of::<ArrayHeader>().checked_add(elements))
        .and_then(|payload| crate::gc::GC_HEADER_SIZE.checked_add(payload))
}
