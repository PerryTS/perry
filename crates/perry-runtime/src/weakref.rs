//! WeakRef and FinalizationRegistry runtime support.
//!
//! Weak target slots are skipped by the GC's strong-edge scanners. A pre-sweep
//! weak pass clears collected WeakRef targets and records pending
//! FinalizationRegistry cleanup jobs after EVERY collection cycle (automatic
//! or explicit `gc()` — 2026-07-09 GC audit: delivery used to be gated on the
//! manual trigger, so ordinary servers never ran any cleanup callback). The
//! recorded jobs are rooted (`scan_pending_finalization_jobs_roots_mut`) and
//! delivered as nextTick jobs: immediately after an explicit `gc()`, or at the
//! next microtask-pump drain for automatic cycles.

use crate::array::{
    js_array_alloc, js_array_get_f64, js_array_length, js_array_push_f64, js_array_set_f64,
    ArrayHeader,
};
use crate::object::{
    js_object_alloc_with_shape, js_object_get_field_by_name, js_object_set_field, ObjectHeader,
};
use crate::value::{
    js_nanbox_get_pointer, JSValue, BIGINT_TAG, POINTER_MASK, POINTER_TAG, STRING_TAG, TAG_MASK,
};
use std::cell::RefCell;

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

const WEAKREF_SHAPE_ID: u32 = 0x7FFF_FE10;
const FINREG_SHAPE_ID: u32 = 0x7FFF_FE11;
const FINREG_RECORD_SHAPE_ID: u32 = 0x7FFF_FE14;
pub const CLASS_ID_WEAKREF: u32 = 0xFFFF_0029;
pub const CLASS_ID_FINALIZATION_REGISTRY: u32 = 0xFFFF_002A;
pub const CLASS_ID_FINALIZATION_RECORD: u32 = 0xFFFF_002B;
/// A single WeakMap/WeakSet entry. Field 0 holds the key — a *weak* slot,
/// skipped by the GC's strong-edge scanners exactly like a WeakRef target or a
/// finalization record's target (see `is_weak_target_trace_slot`). Field 1
/// holds the value (strong; for a WeakSet it is `undefined`). When the key is
/// collected the post-mark pass tombstones both fields to `undefined`, which
/// the lookups treat as an empty slot. Issue #2656.
pub const CLASS_ID_WEAK_ENTRY: u32 = 0xFFFF_002C;

const WEAKREF_TARGET_FIELD: usize = 0;
const FINREG_CALLBACK_FIELD: usize = 0;
const FINREG_ENTRIES_FIELD: usize = 1;
const FINREG_RECORD_TARGET_FIELD: usize = 0;
const FINREG_RECORD_TOKEN_FIELD: usize = 1;
const FINREG_RECORD_HELD_FIELD: usize = 2;
const FINREG_RECORD_PENDING_FIELD: usize = 3;

#[derive(Clone, Copy)]
struct PendingFinalizationJob {
    registry: f64,
    record: f64,
    callback: f64,
    held: f64,
}

thread_local! {
    static PENDING_FINALIZATION_JOBS: RefCell<Vec<PendingFinalizationJob>> =
        const { RefCell::new(Vec::new()) };
    /// Registry of every live weak-target HOLDER object on this thread — a
    /// WeakRef (`CLASS_ID_WEAKREF`), a FinalizationRegistry
    /// (`CLASS_ID_FINALIZATION_REGISTRY`), or a WeakMap/WeakSet entry
    /// (`CLASS_ID_WEAK_ENTRY`) — keyed by the holder's `ObjectHeader` USER
    /// address (the same address the copied-minor pointer classifier and the
    /// arena walk observe). Replaces the old one-way `bool` latch (#6182):
    ///
    /// * `weak_target_holders_allocated()` = "registry non-empty", so a
    ///   program whose only WeakMap died stops paying the copied-minor weak
    ///   cost once its entries are pruned (the bool latched forever).
    /// * `process_weak_targets_from_registry` iterates ONLY these holders
    ///   instead of walking every object in the arena, and classifies weak
    ///   targets with the copy's O(1) page-metadata classifier instead of a
    ///   full-heap `build_valid_pointer_set()` BTreeSet.
    ///
    /// Currency + pruning: `scan_weak_holders_roots_mut` rewrites stored
    /// addresses through evacuation without rooting them; the copied-minor
    /// pass follows forwarding and drops dead holders; the full/fallback
    /// cycles drop dead holders via `prune_dead_weak_holders`.
    static WEAK_HOLDERS: RefCell<crate::fast_hash::PtrHashSet<usize>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_set());
}

/// True while at least one live weak-target holder is registered on this
/// thread. Gates the copied-minor weak-processing pass (see `WEAK_HOLDERS`).
pub(crate) fn weak_target_holders_allocated() -> bool {
    WEAK_HOLDERS.with(|holders| !holders.borrow().is_empty())
}

/// Register a freshly-allocated weak-target holder by its `ObjectHeader` user
/// address (called at the WeakRef / FinalizationRegistry / WeakMap-WeakSet
/// entry alloc sites, after the holder's `class_id` is stamped).
fn weak_holder_register(holder: *const ObjectHeader) {
    WEAK_HOLDERS.with(|holders| {
        holders.borrow_mut().insert(holder as usize);
    });
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WeakWrapperKind {
    WeakRef,
    FinalizationRegistry,
    WeakMap,
    WeakSet,
}

pub(crate) fn weak_wrapper_kind(obj: *const ObjectHeader) -> Option<WeakWrapperKind> {
    if obj.is_null() {
        return None;
    }
    match unsafe { (*obj).class_id } {
        CLASS_ID_WEAKREF => Some(WeakWrapperKind::WeakRef),
        CLASS_ID_FINALIZATION_REGISTRY => Some(WeakWrapperKind::FinalizationRegistry),
        CLASS_ID_WEAKMAP => Some(WeakWrapperKind::WeakMap),
        CLASS_ID_WEAKSET => Some(WeakWrapperKind::WeakSet),
        _ => None,
    }
}

/// The full `util.inspect` body for a weak-collection wrapper, or `None` if
/// `obj` isn't one. Returning the complete string (not just the class name)
/// lets WeakMap/WeakSet print Node's `{ <items unknown> }` placeholder — their
/// contents are intentionally not enumerable — while WeakRef /
/// FinalizationRegistry stay `{}`. Without this, WeakMap/WeakSet leaked their
/// `__perry_wk_entries` storage field (e.g. `{ __perry_wk_entries: [] }`).
pub(crate) fn weak_wrapper_inspect_label(obj: *const ObjectHeader) -> Option<&'static str> {
    match weak_wrapper_kind(obj)? {
        WeakWrapperKind::WeakRef => Some("WeakRef {}"),
        WeakWrapperKind::FinalizationRegistry => Some("FinalizationRegistry {}"),
        WeakWrapperKind::WeakMap => Some("WeakMap { <items unknown> }"),
        WeakWrapperKind::WeakSet => Some("WeakSet { <items unknown> }"),
    }
}

pub(crate) fn weak_collection_entries(obj: *const ObjectHeader) -> Vec<(f64, f64)> {
    match weak_wrapper_kind(obj) {
        Some(WeakWrapperKind::WeakMap | WeakWrapperKind::WeakSet) => {}
        _ => return Vec::new(),
    }

    unsafe {
        let entries_ptr = entries_array(obj as *mut ObjectHeader);
        if entries_ptr.is_null() {
            return Vec::new();
        }
        let len = js_array_length(entries_ptr) as usize;
        let mut entries = Vec::with_capacity(len);
        for i in 0..len {
            let entry = weak_entry_at(entries_ptr, i);
            if entry.is_null() {
                continue;
            }
            let key_bits = object_field_bits(entry, WEAK_ENTRY_KEY_FIELD);
            if key_bits == TAG_UNDEFINED {
                continue; // tombstoned (key collected)
            }
            entries.push((
                f64::from_bits(key_bits),
                f64::from_bits(object_field_bits(entry, WEAK_ENTRY_VALUE_FIELD)),
            ));
        }
        entries
    }
}

fn weakref_type_error(message: &str) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    let err_val = JSValue::pointer(err as *const u8);
    crate::exception::js_throw(f64::from_bits(err_val.bits()))
}

fn is_valid_weak_target(value: f64) -> bool {
    if crate::value::is_js_handle(value) {
        return true;
    }

    // A class reference (e.g. an imported `class Foo {}` passed as a value, or a
    // class prototype) is NaN-boxed as an INT32-tagged (`0x7FFE`) class-ref ID,
    // not a heap pointer. In JS such a value is a function/object and therefore
    // "CanBeHeldWeakly" (ES2023) — a valid WeakMap/WeakSet/WeakRef key. NestJS
    // relies on this: `InitializeOnPreviewAllowlist.add(InternalCoreModule)` does
    // `weakmap.set(InternalCoreModule, true)`, and module-token factories key a
    // WeakMap by the module class. Cross-module class imports arrive as class-ref
    // values (not closure pointers), so without this branch they were rejected
    // with "Invalid value used as weak map key" only inside a full module graph.
    if crate::object::class_ref_id(value).is_some()
        || crate::object::class_prototype_ref_id(value).is_some()
    {
        return true;
    }

    // #1545: a Web Stream handle (ReadableStream/WritableStream/...) does
    // NOT travel as a POINTER_TAG value — it is a raw, un-NaN-boxed finite
    // f64 holding the stream id in the `[0x100000, 0x200000)` band. It still
    // denotes a real stream object, which CanBeHeldWeakly (ES2023): React's
    // SSR `renderToReadableStream` path keys an internal WeakMap by the
    // stream. Without this it threw "Invalid value used as weak map key" and
    // 500'd the Next.js dynamic-SSR routes. The stdlib `stream_handle_probe`
    // confirms a *live registered* stream, so a genuine plain number that
    // merely happens to land in this band (and is not a stream) still
    // throws, matching Node.
    if value.is_finite() && value > 0.0 && value.fract() == 0.0 {
        let id = value as usize;
        if crate::value::addr_class::is_stream_id_band(id) {
            if let Some(probe) = crate::object::stream_handle_probe() {
                if unsafe { probe(id) } {
                    return true;
                }
            }
        }
    }

    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }

    let ptr = (jv.bits() & POINTER_MASK) as usize;
    ptr != 0 && !crate::symbol::is_global_registered_symbol(ptr)
}

fn is_undefined_value(value: f64) -> bool {
    JSValue::from_bits(value.to_bits()).is_undefined()
}

fn is_callable_value(value: f64) -> bool {
    if crate::value::is_js_handle(value) && crate::value::js_handle_is_function(value) {
        return true;
    }

    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }

    crate::closure::is_closure_ptr((jv.bits() & POINTER_MASK) as usize)
}

#[inline]
unsafe fn object_field_slot(obj: *mut ObjectHeader, field_index: usize) -> *mut u64 {
    (obj as *mut u8)
        .add(std::mem::size_of::<ObjectHeader>())
        .cast::<u64>()
        .add(field_index)
}

#[inline]
unsafe fn object_field_bits(obj: *mut ObjectHeader, field_index: usize) -> u64 {
    *object_field_slot(obj, field_index)
}

#[inline]
unsafe fn write_object_field_bits_raw(obj: *mut ObjectHeader, field_index: usize, bits: u64) {
    *object_field_slot(obj, field_index) = bits;
}

#[inline]
fn heap_ptr_from_tagged_bits(bits: u64) -> Option<usize> {
    let tag = bits & TAG_MASK;
    if tag != POINTER_TAG && tag != STRING_TAG && tag != BIGINT_TAG {
        return None;
    }
    let ptr = (bits & POINTER_MASK) as usize;
    (ptr >= 0x1000).then_some(ptr)
}

#[inline]
unsafe fn header_from_user_addr(addr: usize) -> *mut crate::gc::GcHeader {
    (addr as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader
}

#[inline]
unsafe fn value_points_to_gc_type(
    bits: u64,
    valid_ptrs: &crate::gc::ValidPointerSet,
    obj_type: u8,
) -> Option<usize> {
    let ptr = heap_ptr_from_tagged_bits(bits)?;
    if !valid_ptrs.contains(&ptr) {
        return None;
    }
    let header = header_from_user_addr(ptr);
    ((*header).obj_type == obj_type).then_some(ptr)
}

#[inline]
unsafe fn object_value_with_class(
    bits: u64,
    valid_ptrs: &crate::gc::ValidPointerSet,
    class_id: u32,
) -> Option<*mut ObjectHeader> {
    let ptr = value_points_to_gc_type(bits, valid_ptrs, crate::gc::GC_TYPE_OBJECT)?;
    let obj = ptr as *mut ObjectHeader;
    ((*obj).class_id == class_id).then_some(obj)
}

#[inline]
unsafe fn object_value_to_array(
    bits: u64,
    valid_ptrs: &crate::gc::ValidPointerSet,
) -> Option<*mut ArrayHeader> {
    value_points_to_gc_type(bits, valid_ptrs, crate::gc::GC_TYPE_ARRAY)
        .map(|ptr| ptr as *mut ArrayHeader)
}

#[inline]
unsafe fn header_is_live(header: *mut crate::gc::GcHeader) -> bool {
    (*header).gc_flags & (crate::gc::GC_FLAG_MARKED | crate::gc::GC_FLAG_PINNED) != 0
}

fn weak_target_should_clear(
    target_bits: u64,
    valid_ptrs: &crate::gc::ValidPointerSet,
    minor_only: bool,
) -> bool {
    if target_bits == TAG_UNDEFINED {
        return false;
    }
    let target = f64::from_bits(target_bits);
    if crate::value::is_js_handle(target) {
        return false;
    }
    let Some(ptr) = heap_ptr_from_tagged_bits(target_bits) else {
        return false;
    };
    if !valid_ptrs.contains(&ptr) {
        return true;
    }
    if minor_only && !crate::arena::pointer_in_nursery(ptr) {
        return false;
    }
    unsafe {
        let header = header_from_user_addr(ptr);
        !header_is_live(header)
    }
}

/// True when `slot` is a weak target edge and must not be treated as a
/// strong child during mark/remembered-set scans. Rewrite/copy passes should
/// still visit these slots so live weak targets get moved addresses repaired.
pub(crate) unsafe fn is_weak_target_trace_slot(
    header: *mut crate::gc::GcHeader,
    slot: *mut u64,
) -> bool {
    if header.is_null() || (*header).obj_type != crate::gc::GC_TYPE_OBJECT {
        return false;
    }
    let obj = (header as *mut u8).add(crate::gc::GC_HEADER_SIZE) as *mut ObjectHeader;
    match (*obj).class_id {
        // Field 0 is the weak target for both: WeakRef's referent and a
        // WeakMap/WeakSet entry's key.
        CLASS_ID_WEAKREF | CLASS_ID_WEAK_ENTRY => {
            (*obj).field_count > 0 && slot == object_field_slot(obj, 0)
        }
        // A finalization record's target (field 0) AND its unregister token
        // (field 1) are both weak. The spec's [[UnregisterToken]] is an
        // ephemeron-style weak slot; tracing it strongly made the canonical
        // `registry.register(obj, held, obj)` pin the target immortal
        // (2026-07-09 GC audit).
        CLASS_ID_FINALIZATION_RECORD => {
            ((*obj).field_count > 0 && slot == object_field_slot(obj, 0))
                || ((*obj).field_count > 1 && slot == object_field_slot(obj, 1))
        }
        _ => false,
    }
}

/// Allocate a `WeakRef` wrapper object. The target is stored in a normal object
/// field for relocation, but GC mark/remembered-set scans skip that field.
#[no_mangle]
pub extern "C" fn js_weakref_new(target: f64) -> *mut ObjectHeader {
    if !is_valid_weak_target(target) {
        weakref_type_error("WeakRef: invalid target");
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let target_handle = scope.root_nanbox_f64(target);
    let packed = b"__perry_wr_target\0";
    let obj = js_object_alloc_with_shape(WEAKREF_SHAPE_ID, 1, packed.as_ptr(), packed.len() as u32);
    js_object_set_field(
        obj,
        WEAKREF_TARGET_FIELD as u32,
        JSValue::from_bits(target_handle.get_nanbox_u64()),
    );
    unsafe {
        (*obj).class_id = CLASS_ID_WEAKREF;
    }
    weak_holder_register(obj);
    obj
}

/// Return the wrapped value, or `undefined` after the weak target has been
/// cleared by GC.
#[no_mangle]
pub extern "C" fn js_weakref_deref(weakref: f64) -> f64 {
    let ptr = js_nanbox_get_pointer(weakref) as *mut ObjectHeader;
    if ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let key_ptr = crate::string::js_string_from_bytes(b"__perry_wr_target".as_ptr(), 17);
    let val = js_object_get_field_by_name(ptr, key_ptr);
    if val.is_undefined() {
        f64::from_bits(TAG_UNDEFINED)
    } else {
        f64::from_bits(val.bits())
    }
}

/// Allocate a `FinalizationRegistry` wrapper. The first field stores the cleanup
/// callback, the second field stores finalization record objects.
#[no_mangle]
pub extern "C" fn js_finreg_new(callback: f64) -> *mut ObjectHeader {
    if !is_callable_value(callback) {
        weakref_type_error("FinalizationRegistry: cleanup must be callable");
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let callback_handle = scope.root_nanbox_f64(callback);
    // #1766: sentinel-name internal slots so `(fr as any).callback` /
    // `.entries` return `undefined` like Node.
    let packed = b"__perry_fr_callback\0__perry_fr_entries\0";
    let obj = js_object_alloc_with_shape(FINREG_SHAPE_ID, 2, packed.as_ptr(), packed.len() as u32);
    js_object_set_field(
        obj,
        FINREG_CALLBACK_FIELD as u32,
        JSValue::from_bits(callback_handle.get_nanbox_u64()),
    );
    let entries_arr = js_array_alloc(0);
    js_object_set_field(
        obj,
        FINREG_ENTRIES_FIELD as u32,
        JSValue::array_ptr(entries_arr),
    );
    unsafe {
        (*obj).class_id = CLASS_ID_FINALIZATION_REGISTRY;
    }
    // #6182: the FinalizationRegistry itself is the registered holder (not its
    // per-`register()` records). The copied-minor weak pass dispatches on
    // `CLASS_ID_FINALIZATION_REGISTRY` and walks the registry's entries array
    // to reach records — the record has no back-reference to its registry or
    // cleanup callback, so a record-keyed dispatch could not enqueue the
    // cleanup job (the #6192 automatic-cycle behavior). A registry that is
    // created but never `.register()`ed processes an empty entries array (a
    // cheap no-op) and is pruned when it dies.
    weak_holder_register(obj);
    obj
}

fn js_finreg_record_new(target: f64, held: f64, token: f64) -> *mut ObjectHeader {
    let packed = b"__perry_fr_target\0__perry_fr_token\0__perry_fr_held\0__perry_fr_pending\0";
    let record = js_object_alloc_with_shape(
        FINREG_RECORD_SHAPE_ID,
        4,
        packed.as_ptr(),
        packed.len() as u32,
    );
    js_object_set_field(
        record,
        FINREG_RECORD_TARGET_FIELD as u32,
        JSValue::from_bits(target.to_bits()),
    );
    js_object_set_field(
        record,
        FINREG_RECORD_TOKEN_FIELD as u32,
        JSValue::from_bits(token.to_bits()),
    );
    js_object_set_field(
        record,
        FINREG_RECORD_HELD_FIELD as u32,
        JSValue::from_bits(held.to_bits()),
    );
    js_object_set_field(
        record,
        FINREG_RECORD_PENDING_FIELD as u32,
        JSValue::from_bits(TAG_FALSE),
    );
    unsafe {
        (*record).class_id = CLASS_ID_FINALIZATION_RECORD;
    }
    record
}

/// Register a (target, held value, optional token) triple. Returns undefined.
/// The record's target AND token slots are weak (spec: [[UnregisterToken]] is
/// held weakly); only the held value is traced strongly so cleanup delivery
/// remains deterministic. A collected token simply makes a later
/// `unregister(token)` miss, which is spec-correct — the caller can no longer
/// produce that token value anyway.
#[no_mangle]
pub extern "C" fn js_finreg_register(registry: f64, target: f64, held: f64, token: f64) -> f64 {
    if !is_valid_weak_target(target) {
        weakref_type_error("FinalizationRegistry.prototype.register: invalid target");
    }
    if target.to_bits() == held.to_bits() {
        weakref_type_error(
            "FinalizationRegistry.prototype.register: target and holdings must not be same",
        );
    }
    if !is_undefined_value(token) && !is_valid_weak_target(token) {
        weakref_type_error("Invalid unregisterToken");
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let registry_handle = scope.root_nanbox_f64(registry);
    let target_handle = scope.root_nanbox_f64(target);
    let held_handle = scope.root_nanbox_f64(held);
    let token_handle = scope.root_nanbox_f64(token);

    let reg_ptr = js_nanbox_get_pointer(registry_handle.get_nanbox_f64()) as *mut ObjectHeader;
    if reg_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let record = js_finreg_record_new(
        target_handle.get_nanbox_f64(),
        held_handle.get_nanbox_f64(),
        token_handle.get_nanbox_f64(),
    );
    let record_val = f64::from_bits(JSValue::pointer(record as *const u8).bits());
    let record_handle = scope.root_nanbox_f64(record_val);
    let reg_ptr = js_nanbox_get_pointer(registry_handle.get_nanbox_f64()) as *mut ObjectHeader;
    let entries_key = crate::string::js_string_from_bytes(b"__perry_fr_entries".as_ptr(), 18);
    let entries_val = js_object_get_field_by_name(reg_ptr, entries_key);
    let entries_ptr = (entries_val.bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ArrayHeader;
    if entries_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let entries_ptr = js_array_push_f64(entries_ptr, record_handle.get_nanbox_f64());
    let reg_ptr = js_nanbox_get_pointer(registry_handle.get_nanbox_f64()) as *mut ObjectHeader;
    js_object_set_field(
        reg_ptr,
        FINREG_ENTRIES_FIELD as u32,
        JSValue::array_ptr(entries_ptr),
    );
    f64::from_bits(TAG_UNDEFINED)
}

/// Unregister all entries matching the given token. Returns `true` if at least
/// one entry was found and removed, `false` otherwise. Token comparison uses
/// strict equality (raw NaN-box bit comparison) which is correct for object
/// references — both sides are stored as POINTER_TAG-tagged f64 values.
#[no_mangle]
pub extern "C" fn js_finreg_unregister(registry: f64, token: f64) -> f64 {
    if !is_valid_weak_target(token) {
        weakref_type_error("Invalid unregisterToken");
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let registry_handle = scope.root_nanbox_f64(registry);
    let token_handle = scope.root_nanbox_f64(token);

    let reg_ptr = js_nanbox_get_pointer(registry_handle.get_nanbox_f64()) as *mut ObjectHeader;
    if reg_ptr.is_null() {
        return f64::from_bits(TAG_FALSE);
    }
    let entries_key = crate::string::js_string_from_bytes(b"__perry_fr_entries".as_ptr(), 18);
    let entries_val = js_object_get_field_by_name(reg_ptr, entries_key);
    let entries_ptr = (entries_val.bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ArrayHeader;
    if entries_ptr.is_null() {
        return f64::from_bits(TAG_FALSE);
    }
    let len = js_array_length(entries_ptr) as usize;
    let mut found = false;
    // Rebuild the entries array without matching records.
    let new_arr_handle = scope.root_raw_mut_ptr(js_array_alloc(len as u32));
    let reg_ptr = js_nanbox_get_pointer(registry_handle.get_nanbox_f64()) as *mut ObjectHeader;
    let entries_key = crate::string::js_string_from_bytes(b"__perry_fr_entries".as_ptr(), 18);
    let entries_val = js_object_get_field_by_name(reg_ptr, entries_key);
    let entries_ptr = (entries_val.bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ArrayHeader;
    if entries_ptr.is_null() {
        return f64::from_bits(TAG_FALSE);
    }
    let len = js_array_length(entries_ptr) as usize;
    let token_bits = token_handle.get_nanbox_u64();
    for i in 0..len {
        let record_val = js_array_get_f64(entries_ptr, i as u32);
        let record_ptr = (record_val.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader;
        if record_ptr.is_null() {
            continue;
        }
        let stored_token = unsafe { object_field_bits(record_ptr, FINREG_RECORD_TOKEN_FIELD) };
        if stored_token == token_bits {
            found = true;
            continue;
        }
        let pushed = js_array_push_f64(new_arr_handle.get_raw_mut_ptr(), record_val);
        new_arr_handle.set_raw_mut_ptr(pushed);
    }
    // Replace entries field with the new array.
    let reg_ptr = js_nanbox_get_pointer(registry_handle.get_nanbox_f64()) as *mut ObjectHeader;
    js_object_set_field(
        reg_ptr,
        FINREG_ENTRIES_FIELD as u32,
        JSValue::array_ptr(new_arr_handle.get_raw_mut_ptr()),
    );
    if found {
        f64::from_bits(TAG_TRUE)
    } else {
        f64::from_bits(TAG_FALSE)
    }
}

/// Number of recorded-but-undelivered FinalizationRegistry cleanup jobs on
/// this thread. Non-zero between a collection's weak pass and the next
/// delivery point (explicit-`gc()` tail or microtask-pump drain).
pub(crate) fn pending_finalization_jobs_count() -> usize {
    PENDING_FINALIZATION_JOBS.with(|jobs| jobs.borrow().len())
}

/// Microtask-pump drain: convert jobs recorded by AUTOMATIC collection cycles
/// into nextTick callback invocations. Returns the number of jobs delivered so
/// the pump can count them as work. The jobs vec is `mem::take`n by the
/// delivery, so a manual `gc()` that already delivered leaves nothing here
/// (no double-delivery), and re-entrant pump drains are safe.
pub(crate) fn drain_pending_finalization_jobs() -> i32 {
    let pending = pending_finalization_jobs_count();
    if pending == 0 {
        return 0;
    }
    queue_pending_finalization_callbacks_after_gc();
    pending as i32
}

pub(crate) fn scan_pending_finalization_jobs_roots_mut(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
) {
    PENDING_FINALIZATION_JOBS.with(|jobs| {
        for job in jobs.borrow_mut().iter_mut() {
            visitor.visit_nanbox_f64_slot(&mut job.registry);
            visitor.visit_nanbox_f64_slot(&mut job.record);
            visitor.visit_nanbox_f64_slot(&mut job.callback);
            visitor.visit_nanbox_f64_slot(&mut job.held);
        }
    });
}

// =============================================================================
// Weak-target liveness abstraction (#6182)
// =============================================================================
//
// The per-holder tombstone helpers (`process_weakref_after_mark` et al.) are
// shared between two passes that decide "was this weak target collected?"
// differently:
//
// * The FULL / fallback cycle (`process_weak_targets_after_mark`, driven from
//   cycle.rs `WeakProcessing`) probes the `ValidPointerSet` it already built
//   for its main trace. UNCHANGED behavior — see `weak_target_should_clear`.
// * The copied-minor fast path (`process_weak_targets_from_registry`) probes
//   the copy's O(1) page-metadata classifier (`CopyingPointerSet`), avoiding
//   both the full-heap BTreeSet build and the whole-arena walk. See
//   `weak_target_should_clear_copied`.
//
// The helpers are parameterized over `&dyn WeakLiveness` so neither pass's
// liveness logic can drift from the other's tombstone/enqueue/token semantics.

trait WeakLiveness {
    /// True when the weak target denoted by `target_bits` was collected and
    /// its slot must be tombstoned to `undefined`.
    fn target_should_clear(&self, target_bits: u64) -> bool;
    /// Resolve `bits` to a LIVE `GC_TYPE_ARRAY` header (a FinalizationRegistry
    /// entries array), or `None` if dead/not-an-array.
    ///
    /// # Safety
    /// `bits` must be a value word read from a live holder's field.
    unsafe fn as_live_array(&self, bits: u64) -> Option<*mut ArrayHeader>;
    /// Resolve `bits` to a LIVE `GC_TYPE_OBJECT` of `class_id` (a
    /// FinalizationRegistry record), or `None`.
    ///
    /// # Safety
    /// `bits` must be a value word read from a live holder's field.
    unsafe fn as_live_object_with_class(
        &self,
        bits: u64,
        class_id: u32,
    ) -> Option<*mut ObjectHeader>;
}

/// Full / fallback-cycle liveness: the existing `ValidPointerSet`-based
/// predicates, behavior-identical to pre-#6182 code.
struct FullCycleLiveness<'a> {
    valid_ptrs: &'a crate::gc::ValidPointerSet,
    minor_only: bool,
}

impl WeakLiveness for FullCycleLiveness<'_> {
    fn target_should_clear(&self, target_bits: u64) -> bool {
        weak_target_should_clear(target_bits, self.valid_ptrs, self.minor_only)
    }
    unsafe fn as_live_array(&self, bits: u64) -> Option<*mut ArrayHeader> {
        object_value_to_array(bits, self.valid_ptrs)
    }
    unsafe fn as_live_object_with_class(
        &self,
        bits: u64,
        class_id: u32,
    ) -> Option<*mut ObjectHeader> {
        object_value_with_class(bits, self.valid_ptrs, class_id)
    }
}

/// Copied-minor liveness: probe the copy's page-metadata classifier. Callers
/// pass value words that have ALREADY been made current — weak target slots by
/// `repair_weak_slots`, holder strong fields (entries array, records) by the
/// collector's evacuation rewrite — so this does not itself follow forwarding.
struct CopiedMinorLiveness<'a> {
    ptrs: &'a crate::gc::CopyingPointerSet,
}

impl WeakLiveness for CopiedMinorLiveness<'_> {
    fn target_should_clear(&self, target_bits: u64) -> bool {
        weak_target_should_clear_copied(target_bits, self.ptrs)
    }
    unsafe fn as_live_array(&self, bits: u64) -> Option<*mut ArrayHeader> {
        classify_gc_type_child(bits, self.ptrs, crate::gc::GC_TYPE_ARRAY)
            .map(|ptr| ptr as *mut ArrayHeader)
    }
    unsafe fn as_live_object_with_class(
        &self,
        bits: u64,
        class_id: u32,
    ) -> Option<*mut ObjectHeader> {
        let ptr = classify_gc_type_child(bits, self.ptrs, crate::gc::GC_TYPE_OBJECT)?;
        let obj = ptr as *mut ObjectHeader;
        ((*obj).class_id == class_id).then_some(obj)
    }
}

/// Copied-minor twin of `weak_target_should_clear`. A weak target is collected
/// iff it is a REAL, NURSERY heap object the classifier can attribute and that
/// object is not live (MARKED|PINNED). Two correctness rules, both replicating
/// the `weak_target_should_clear(.., minor_only = true)` semantics:
///
/// * A classifier miss (`None`) means "not a collectible heap object" — a
///   handle-band id (Proxy / node:http / fetch / Web-Stream) or any non-heap
///   pointer — so KEEP it. The old `ValidPointerSet` predicate treated "not in
///   valid_ptrs" as dead, which FALSE-tombstoned live handle-band weak keys
///   ≥0x1000 on the first GC (2026-07-09 audit §8); this path fixes it.
/// * A copied minor IS a MINOR, so it does NOT mark the old generation (old
///   objects are black leaves, not re-traced). An old-gen / longlived / malloc
///   target is therefore live-but-unmarked here — its mark bit proves nothing —
///   so it must be conservatively KEPT (a full GC tombstones it properly once it
///   traces old-gen). This mirrors the original `minor_only && !pointer_in_nursery`
///   guard: without it a WeakMap key that survived enough minors to be PROMOTED
///   to old-gen would be silently dropped from the map on the next copied minor.
///   Only nursery/survivor targets — which this minor DID trace — may be judged
///   dead by their mark bit.
fn weak_target_should_clear_copied(target_bits: u64, ptrs: &crate::gc::CopyingPointerSet) -> bool {
    if target_bits == TAG_UNDEFINED {
        return false;
    }
    let target = f64::from_bits(target_bits);
    if crate::value::is_js_handle(target) {
        return false;
    }
    let Some(ptr) = heap_ptr_from_tagged_bits(target_bits) else {
        return false;
    };
    match ptrs.classify(ptr) {
        // Band-id handle / non-heap pointer → not collectible → keep.
        None => false,
        Some(cp) => {
            // Old / longlived / malloc target in a minor: unmarked ≠ dead → keep.
            if !crate::arena::pointer_in_nursery(ptr) {
                return false;
            }
            // Nursery/survivor target this minor traced: dead iff not MARKED|PINNED.
            unsafe { !header_is_live(cp.header) }
        }
    }
}

/// Resolve `bits` to a heap object of `obj_type` using the copied-minor
/// classifier, without following forwarding (the caller's fields are already
/// current). Returns the user address, or `None` if not a valid heap object of
/// that type.
///
/// This intentionally does NOT gate on the mark bit: it resolves a STRONG child
/// of a holder whose liveness `resolve_live_holder_copied` already established
/// (a FinalizationRegistry's entries array and records). Requiring MARKED would
/// wrongly drop an entries array or record that was PROMOTED to old-gen (a minor
/// doesn't mark old-gen), delaying finalization — the original
/// `object_value_to_array` / `object_value_with_class` resolvers only checked
/// validity + `obj_type`, never MARKED.
unsafe fn classify_gc_type_child(
    bits: u64,
    ptrs: &crate::gc::CopyingPointerSet,
    obj_type: u8,
) -> Option<usize> {
    let ptr = heap_ptr_from_tagged_bits(bits)?;
    let cp = ptrs.classify(ptr)?;
    (unsafe { (*cp.header).obj_type } == obj_type).then_some(ptr)
}

/// What the copied-minor pass should do with a registered holder address.
enum HolderDisposition {
    /// Live holder scanned this cycle (its weak slots are repaired): rekey the
    /// registry to this current address and process it.
    Process(usize),
    /// Cannot be proven dead in a minor (an unmarked OLD/longlived holder — a
    /// minor doesn't mark old-gen) AND its weak slots may be stale/unrepaired:
    /// leave it registered untouched and let a full GC resolve it. Mirrors the
    /// original arena walk, which only ever processed MARKED objects.
    Keep,
    /// Provably dead (unmarked nursery holder) or unclassifiable (stale /
    /// recycled address): remove it from the registry.
    Drop,
}

/// Decide the disposition of a registered holder in a copied minor. Copied-minor
/// only: it reads pre-flip from-space headers, so the FORWARDED bit and the
/// forwarding address are still valid.
///
/// A copied minor is a MINOR — it authoritatively traces only the nursery and
/// does NOT mark or scan the old generation. So:
/// * a live young holder is EVACUATED (FORWARDED); its to-space copy carries
///   MARKED and had its weak slots repaired this cycle → `Process`;
/// * an unmarked NURSERY holder is genuinely dead (an eligible copied minor has
///   no pinned young survivors) → `Drop`;
/// * an unmarked OLD/longlived holder is live-but-unmarked and its weak slots
///   were neither repaired nor remembered, so it can be neither proven dead nor
///   safely processed here → `Keep` (a full GC handles it).
unsafe fn resolve_weak_holder_copied(
    ptrs: &crate::gc::CopyingPointerSet,
    addr: usize,
) -> HolderDisposition {
    let Some(cp) = ptrs.classify(addr) else {
        return HolderDisposition::Drop;
    };
    let (cur_addr, cur_header) = if (*cp.header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
        let fwd = crate::gc::forwarding_address(cp.header) as usize;
        match ptrs.classify(fwd) {
            Some(cp2) => (fwd, cp2.header),
            None => return HolderDisposition::Drop,
        }
    } else {
        (addr, cp.header)
    };
    if header_is_live(cur_header) {
        // MARKED|PINNED ⇒ scanned this cycle ⇒ weak slots repaired ⇒ processable.
        return HolderDisposition::Process(cur_addr);
    }
    if crate::arena::pointer_in_nursery(cur_addr) {
        HolderDisposition::Drop
    } else {
        HolderDisposition::Keep
    }
}

/// Full / fallback-cycle weak processing. Walks EVERY live object in the arena
/// to find the three weak-holder class_ids and tombstones dead weak targets
/// using the `ValidPointerSet` the caller built for its main trace. UNCHANGED
/// by #6182 (the registry optimization is copied-minor-only).
pub(crate) fn process_weak_targets_after_mark(
    valid_ptrs: &crate::gc::ValidPointerSet,
    minor_only: bool,
    enqueue_callbacks: bool,
) {
    // #6180 pause floor: the whole-heap walk below exists only to FIND weak
    // holders (WeakRef / FinalizationRegistry / WeakMap-entry objects). The
    // #6182 registry tracks every live holder — if none exist (the common
    // case), the entire O(heap) pass is a no-op. This is the single largest
    // atomic-finalize cost for weakref-free programs.
    if !weak_target_holders_allocated() {
        return;
    }
    let liveness = FullCycleLiveness {
        valid_ptrs,
        minor_only,
    };
    crate::arena::arena_walk_objects(|header_ptr| unsafe {
        let header = header_ptr as *mut crate::gc::GcHeader;
        if (*header).obj_type != crate::gc::GC_TYPE_OBJECT || !header_is_live(header) {
            return;
        }
        let obj = header_ptr.add(crate::gc::GC_HEADER_SIZE) as *mut ObjectHeader;
        dispatch_weak_holder(obj, &liveness, enqueue_callbacks);
    });
}

/// Copied-minor weak processing (#6182). Iterates ONLY the registered holders
/// — following each through evacuation and dropping dead ones — instead of
/// walking the whole arena, and tombstones dead weak targets with the O(1)
/// page-metadata classifier instead of a full-heap valid-pointer BTreeSet.
///
/// Runs at the copied-minor completion point AFTER `repair_weak_slots` and
/// BEFORE the from-space flip, so weak target slots and holder strong fields
/// are already current and dead holders' from-space headers are still intact.
pub(crate) fn process_weak_targets_from_registry(
    ptrs: &crate::gc::CopyingPointerSet,
    enqueue_callbacks: bool,
) {
    let liveness = CopiedMinorLiveness { ptrs };
    // Snapshot addresses so the registry can be mutated (rekey moved holders,
    // drop dead ones) while iterating. Holders don't allocate GC objects in
    // the helpers below, so no re-entrant `WEAK_HOLDERS` borrow occurs.
    let snapshot: Vec<usize> =
        WEAK_HOLDERS.with(|holders| holders.borrow().iter().copied().collect());
    for addr in snapshot {
        match unsafe { resolve_weak_holder_copied(ptrs, addr) } {
            HolderDisposition::Drop => {
                WEAK_HOLDERS.with(|holders| {
                    holders.borrow_mut().remove(&addr);
                });
            }
            // Live old holder: keep tracking it, but do not process it this
            // minor (a full GC will). Not dropping keeps a promoted holder from
            // being forgotten by the registry.
            HolderDisposition::Keep => {}
            HolderDisposition::Process(current) => {
                if current != addr {
                    WEAK_HOLDERS.with(|holders| {
                        let mut holders = holders.borrow_mut();
                        holders.remove(&addr);
                        holders.insert(current);
                    });
                }
                unsafe {
                    dispatch_weak_holder(
                        current as *mut ObjectHeader,
                        &liveness,
                        enqueue_callbacks,
                    );
                }
            }
        }
    }
}

/// Dispatch a single live weak holder (WeakRef / FinalizationRegistry /
/// WeakMap-WeakSet entry) to its tombstone helper. Shared by both passes; the
/// liveness strategy is the only thing that differs.
#[inline]
unsafe fn dispatch_weak_holder(
    obj: *mut ObjectHeader,
    liveness: &dyn WeakLiveness,
    enqueue_callbacks: bool,
) {
    match (*obj).class_id {
        CLASS_ID_WEAKREF => process_weakref_after_mark(obj, liveness),
        CLASS_ID_FINALIZATION_REGISTRY => {
            process_finreg_after_mark(obj, liveness, enqueue_callbacks)
        }
        // Each WeakMap/WeakSet entry is its own GcHeader-backed object; the
        // weak key slot's address is repaired by the copy/rewrite pass before
        // this pass reads it.
        CLASS_ID_WEAK_ENTRY => process_weak_entry_after_mark(obj, liveness),
        _ => {}
    }
}

/// Mutable-root scanner keeping the `WEAK_HOLDERS` addresses current across
/// evacuation (#6182). Metadata-only: `visit_metadata_usize_slot` rewrites a
/// forwarded address in rewrite/verify phases and emits NOTHING during mark, so
/// registering this scanner never keeps a dead holder alive. Copied-minor
/// currency is handled independently by `process_weak_targets_from_registry`
/// (the GC test harness clears the scanner registry); this covers full-cycle
/// evacuation, where the registered scanners ARE the rewrite mechanism.
pub(crate) fn scan_weak_holders_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    if !visitor.is_metadata_rewrite_phase() {
        return;
    }
    WEAK_HOLDERS.with(|holders| {
        let mut holders = holders.borrow_mut();
        if holders.is_empty() {
            return;
        }
        // Rebuild only when an address actually moved — a `HashSet` can't
        // rewrite keys in place. Mirrors `scan_descriptor_roots_mut`.
        let needs_rebuild = holders
            .iter()
            .any(|&addr| rewritten_holder_addr(visitor, addr) != addr);
        if needs_rebuild {
            let old = std::mem::take(&mut *holders);
            let mut rebuilt = crate::fast_hash::new_ptr_hash_set();
            for addr in old {
                rebuilt.insert(rewritten_holder_addr(visitor, addr));
            }
            *holders = rebuilt;
        }
    });
}

#[inline]
fn rewritten_holder_addr(visitor: &mut crate::gc::RuntimeRootVisitor<'_>, addr: usize) -> usize {
    let mut a = addr;
    visitor.visit_metadata_usize_slot(&mut a);
    a
}

/// Drop dead weak holders from the registry (#6182). Used by the full /
/// fallback (non-copying) cycles via `dead_owner::prune_dead_owner_side_tables_post_trace`;
/// the copied-minor path prunes inline in `process_weak_targets_from_registry`.
/// Keeping the registry pruned lets `weak_target_holders_allocated()` return to
/// zero once a transient WeakMap and its entries die.
pub(crate) fn prune_dead_weak_holders(is_dead: &dyn Fn(usize) -> bool) {
    WEAK_HOLDERS.with(|holders| {
        let mut holders = holders.borrow_mut();
        if holders.is_empty() {
            return;
        }
        holders.retain(|&addr| !is_dead(addr));
    });
}

unsafe fn process_weakref_after_mark(obj: *mut ObjectHeader, liveness: &dyn WeakLiveness) {
    let target_bits = object_field_bits(obj, WEAKREF_TARGET_FIELD);
    if liveness.target_should_clear(target_bits) {
        write_object_field_bits_raw(obj, WEAKREF_TARGET_FIELD, TAG_UNDEFINED);
    }
}

/// A live WeakMap/WeakSet entry whose key was collected is tombstoned: both the
/// key and the value slots are set to `undefined` so the value becomes
/// collectible (next cycle) and the lookups skip the slot. The entry object
/// itself is reclaimed when `delete`/`set` next compacts the entries array (or
/// when the whole collection dies). Mirrors `process_weakref_after_mark`.
unsafe fn process_weak_entry_after_mark(entry: *mut ObjectHeader, liveness: &dyn WeakLiveness) {
    let key_bits = object_field_bits(entry, WEAK_ENTRY_KEY_FIELD);
    if liveness.target_should_clear(key_bits) {
        write_object_field_bits_raw(entry, WEAK_ENTRY_KEY_FIELD, TAG_UNDEFINED);
        write_object_field_bits_raw(entry, WEAK_ENTRY_VALUE_FIELD, TAG_UNDEFINED);
    }
}

unsafe fn process_finreg_after_mark(
    registry: *mut ObjectHeader,
    liveness: &dyn WeakLiveness,
    enqueue_callbacks: bool,
) {
    let callback = f64::from_bits(object_field_bits(registry, FINREG_CALLBACK_FIELD));
    let entries_bits = object_field_bits(registry, FINREG_ENTRIES_FIELD);
    let Some(entries) = liveness.as_live_array(entries_bits) else {
        return;
    };
    let len = js_array_length(entries) as usize;
    let registry_value = f64::from_bits(JSValue::pointer(registry as *const u8).bits());
    for i in 0..len {
        let record_value = js_array_get_f64(entries, i as u32);
        let Some(record) = liveness
            .as_live_object_with_class(record_value.to_bits(), CLASS_ID_FINALIZATION_RECORD)
        else {
            continue;
        };
        process_finreg_record_after_mark(
            registry_value,
            record,
            callback,
            liveness,
            enqueue_callbacks,
        );
    }
}

unsafe fn process_finreg_record_after_mark(
    registry: f64,
    record: *mut ObjectHeader,
    callback: f64,
    liveness: &dyn WeakLiveness,
    enqueue_callbacks: bool,
) {
    let pending_bits = object_field_bits(record, FINREG_RECORD_PENDING_FIELD);
    let target_bits = object_field_bits(record, FINREG_RECORD_TARGET_FIELD);
    let collected = liveness.target_should_clear(target_bits);
    if collected {
        write_object_field_bits_raw(record, FINREG_RECORD_TARGET_FIELD, TAG_UNDEFINED);
        write_object_field_bits_raw(record, FINREG_RECORD_PENDING_FIELD, TAG_TRUE);
    }
    // The unregister token is a weak slot too (see `is_weak_target_trace_slot`):
    // tombstone it when its referent died so `unregister(deadToken)` misses and
    // the record can't resurrect the token graph. Independent of the target —
    // spec: clearing [[UnregisterToken]] does not cancel the registration.
    let token_bits = object_field_bits(record, FINREG_RECORD_TOKEN_FIELD);
    if liveness.target_should_clear(token_bits) {
        write_object_field_bits_raw(record, FINREG_RECORD_TOKEN_FIELD, TAG_UNDEFINED);
    }

    if enqueue_callbacks && (pending_bits == TAG_TRUE || collected) {
        let held = f64::from_bits(object_field_bits(record, FINREG_RECORD_HELD_FIELD));
        let record_value = f64::from_bits(JSValue::pointer(record as *const u8).bits());
        PENDING_FINALIZATION_JOBS.with(|jobs| {
            jobs.borrow_mut().push(PendingFinalizationJob {
                registry,
                record: record_value,
                callback,
                held,
            });
        });
        write_object_field_bits_raw(record, FINREG_RECORD_PENDING_FIELD, TAG_FALSE);
    }
}

pub(crate) fn queue_pending_finalization_callbacks_after_gc() {
    let jobs = PENDING_FINALIZATION_JOBS.with(|jobs| std::mem::take(&mut *jobs.borrow_mut()));
    for job in jobs {
        let scope = crate::gc::RuntimeHandleScope::new();
        let registry = scope.root_nanbox_f64(job.registry);
        let record = scope.root_nanbox_f64(job.record);
        let callback = scope.root_nanbox_f64(job.callback);
        let held = scope.root_nanbox_f64(job.held);
        let callback_ptr = js_nanbox_get_pointer(callback.get_nanbox_f64()) as usize;
        if callback_ptr >= 0x1000 && crate::closure::is_closure_ptr(callback_ptr) {
            let held_arg = held.get_nanbox_f64();
            unsafe {
                crate::builtins::js_queue_next_tick_args(callback_ptr as i64, &held_arg, 1);
            }
        }
        remove_finalization_record_from_registry(
            registry.get_nanbox_f64(),
            record.get_nanbox_f64(),
        );
    }
}

fn remove_finalization_record_from_registry(registry: f64, record: f64) {
    let reg_ptr = js_nanbox_get_pointer(registry) as *mut ObjectHeader;
    if reg_ptr.is_null() {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let registry_handle = scope.root_nanbox_f64(registry);
    let record_handle = scope.root_nanbox_f64(record);
    let reg_ptr = js_nanbox_get_pointer(registry_handle.get_nanbox_f64()) as *mut ObjectHeader;
    if reg_ptr.is_null() {
        return;
    }
    let entries_key = crate::string::js_string_from_bytes(b"__perry_fr_entries".as_ptr(), 18);
    let entries_val = js_object_get_field_by_name(reg_ptr, entries_key);
    let entries_ptr = (entries_val.bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ArrayHeader;
    if entries_ptr.is_null() {
        return;
    }
    let len = js_array_length(entries_ptr) as usize;
    let new_arr_handle = scope.root_raw_mut_ptr(js_array_alloc(len as u32));
    let reg_ptr = js_nanbox_get_pointer(registry_handle.get_nanbox_f64()) as *mut ObjectHeader;
    let entries_key = crate::string::js_string_from_bytes(b"__perry_fr_entries".as_ptr(), 18);
    let entries_val = js_object_get_field_by_name(reg_ptr, entries_key);
    let entries_ptr = (entries_val.bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ArrayHeader;
    if entries_ptr.is_null() {
        return;
    }
    let record_bits = record_handle.get_nanbox_f64().to_bits();
    let len = js_array_length(entries_ptr) as usize;
    for i in 0..len {
        let current = js_array_get_f64(entries_ptr, i as u32);
        if current.to_bits() == record_bits {
            continue;
        }
        let pushed = js_array_push_f64(new_arr_handle.get_raw_mut_ptr(), current);
        new_arr_handle.set_raw_mut_ptr(pushed);
    }
    let reg_ptr = js_nanbox_get_pointer(registry_handle.get_nanbox_f64()) as *mut ObjectHeader;
    js_object_set_field(
        reg_ptr,
        FINREG_ENTRIES_FIELD as u32,
        JSValue::array_ptr(new_arr_handle.get_raw_mut_ptr()),
    );
}

// =============================================================================
// WeakMap / WeakSet runtime — implemented separately from `crate::map`/`crate::set`
// because the existing `js_map_set` does *content-based* equality on string-like
// pointer keys, which incorrectly collapses two distinct empty objects (`{}`)
// onto the same slot. WeakMap/WeakSet require *reference* equality, so we use
// our own storage backed by an `entries` array of `[key, value]` pairs (set just
// stores `[key, key]`) with raw NaN-box bit comparison.
// =============================================================================

const WEAKMAP_SHAPE_ID: u32 = 0x7FFF_FE12;
const WEAKSET_SHAPE_ID: u32 = 0x7FFF_FE13;
const WEAK_ENTRY_SHAPE_ID: u32 = 0x7FFF_FE15;

const WEAK_ENTRY_KEY_FIELD: usize = 0;
const WEAK_ENTRY_VALUE_FIELD: usize = 1;

/// Allocate a WeakMap/WeakSet entry object (`CLASS_ID_WEAK_ENTRY`). Field 0 is
/// the key — a weak slot the GC's strong scanners skip (see
/// `is_weak_target_trace_slot`), so a key reachable only through the collection
/// is collectible. Field 1 is the value, traced strongly while the key is live.
fn weak_entry_new(key: f64, value: f64) -> *mut ObjectHeader {
    // Sentinel-named slots so `(entry as any).key` can't leak storage and the
    // names never collide with user fields.
    let packed = b"__perry_we_key\0__perry_we_value\0";
    let entry =
        js_object_alloc_with_shape(WEAK_ENTRY_SHAPE_ID, 2, packed.as_ptr(), packed.len() as u32);
    js_object_set_field(
        entry,
        WEAK_ENTRY_KEY_FIELD as u32,
        JSValue::from_bits(key.to_bits()),
    );
    js_object_set_field(
        entry,
        WEAK_ENTRY_VALUE_FIELD as u32,
        JSValue::from_bits(value.to_bits()),
    );
    // Stamp the weak-entry class_id last (mirrors js_weakref_new) so the GC's
    // weak-slot recognition keys off it on the next mark.
    unsafe {
        (*entry).class_id = CLASS_ID_WEAK_ENTRY;
    }
    weak_holder_register(entry);
    entry
}

/// Read the entry-object pointer stored at `entries[i]`, or null. Entries hold
/// `CLASS_ID_WEAK_ENTRY` object pointers (POINTER_TAG); the low 48 bits are the
/// address regardless of tag.
#[inline]
unsafe fn weak_entry_at(entries: *mut ArrayHeader, i: usize) -> *mut ObjectHeader {
    let v = js_array_get_f64(entries, i as u32);
    (v.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader
}

// Reserved `ObjectHeader.class_id` markers for WeakMap/WeakSet instances.
// These follow the same `0xFFFF00xx` reserved-builtin convention as
// CLASS_ID_MAP/CLASS_ID_SET (see object/instanceof.rs). Unlike Map/Set —
// which are plain-alloc and tracked in raw-pointer registries — WeakMap/
// WeakSet objects are GcHeader-backed and movable, so a registry of raw
// pointers would dangle after a GC evacuation. The class_id travels with
// the object across GC moves, so `js_native_call_method` can recognise a
// WeakMap/WeakSet held in an `any`-typed binding (e.g. effect's
// `globalValue(() => new WeakMap())`) and dispatch .has/.get/.set/.delete/
// .add through to these helpers. 0x27/0x28 are the next free slots after
// CLASS_ID_BLOB (0x26). Issue #1757/#1758.
pub const CLASS_ID_WEAKMAP: u32 = 0xFFFF_0027;
pub const CLASS_ID_WEAKSET: u32 = 0xFFFF_0028;

/// Sentinel name of the internal slot-0 field that backs a `WeakMap`/`WeakSet`
/// with its `[k, v]`-pair entry array (`js_weakmap_new` / `js_weakset_new`).
/// `WeakMap`/`WeakSet` are `GC_TYPE_OBJECT`s, so this is an own enumerable
/// string key — it must NEVER surface through any enumeration surface
/// (`Object.keys` / `Object.assign` / spread / `JSON.stringify` / `for…in` /
/// `hasOwnProperty`). Hidden via `is_internal_runtime_key_bytes`, exactly like
/// the `class … extends Map/Set` backing key. Internal reads go through
/// `entries_array` by direct name lookup, so hiding it from enumeration is
/// safe. Refs #6120.
pub(crate) const WEAK_ENTRIES_KEY: &[u8] = b"__perry_wk_entries";

/// Dynamic-dispatch entry point for WeakMap/WeakSet method calls (issue
/// #1757/#1758). `js_native_call_method` calls this for any heap object;
/// it returns `Some(result)` only when `obj` carries the reserved
/// WeakMap/WeakSet `class_id` and `method_name` is one of *that class's own*
/// methods, and `None` otherwise so the caller falls through to its normal
/// dispatch. `receiver` is the NaN-boxed f64 the `js_weak*` helpers expect.
///
/// A method that isn't one of the receiver's own (e.g. `"add"` on a WeakMap,
/// or any name outside `set`/`add`/`get`/`has`/`delete`) falls through to
/// `None` so the ordinary property lookup resolves it — correctly missing
/// and raising `TypeError: ... is not a function` on a call, rather than
/// this function silently answering `undefined`.
///
/// # Safety
/// `obj` must be a valid, readable `ObjectHeader` pointer (the caller has
/// already validated it as a live heap object).
pub unsafe fn try_weak_method_dispatch(
    obj: *const ObjectHeader,
    receiver: f64,
    method_name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    let class_id = (*obj).class_id;
    if class_id != CLASS_ID_WEAKMAP && class_id != CLASS_ID_WEAKSET {
        return None;
    }
    let args: &[f64] = if !args_ptr.is_null() && args_len > 0 {
        std::slice::from_raw_parts(args_ptr, args_len)
    } else {
        &[]
    };
    // #5834: dispatch regardless of arg count, padding missing positions with
    // `undefined` — mirrors calling the real thunks reflectively. Arity-gating
    // these arms let `s.add()` (zero args) fall through to a no-op, skipping
    // `js_weakset_add`'s CanBeHeldWeakly check entirely (it must throw
    // `TypeError` since `undefined` cannot be held weakly).
    //
    // Also gate each method by the receiver's actual class: `"set"`/`"get"`
    // only exist on WeakMap, `"add"` only on WeakSet (`"has"`/`"delete"` are
    // shared). Without this a WeakMap receiver could reach `js_weakset_add`
    // for a `.add(...)` call (and vice versa) instead of falling through to
    // the ordinary property lookup, which correctly resolves the missing
    // method to `undefined` and throws `TypeError: ... is not a function`.
    let undef = f64::from_bits(TAG_UNDEFINED);
    let arg = |i: usize| args.get(i).copied().unwrap_or(undef);
    let result = match (method_name, class_id) {
        ("set", CLASS_ID_WEAKMAP) => js_weakmap_set(receiver, arg(0), arg(1)),
        ("add", CLASS_ID_WEAKSET) => js_weakset_add(receiver, arg(0)),
        ("get", CLASS_ID_WEAKMAP) => js_weakmap_get(receiver, arg(0)),
        ("has", _) => js_weakmap_has(receiver, arg(0)),
        ("delete", _) => js_weakmap_delete(receiver, arg(0)),
        _ => return None,
    };
    Some(result)
}

/// Return the reserved WeakMap/WeakSet `class_id` of `receiver` if it is one
/// of those collections, else `None`. Backs the reflective
/// `WeakMap.prototype.*` / `WeakSet.prototype.*` thunks so they can perform
/// the spec brand check (`TypeError` on a non-Weak* receiver) before
/// dispatching. The `GcHeader.obj_type == GC_TYPE_OBJECT` pre-filter ensures
/// the pointer is an `ObjectHeader`-backed allocation before `class_id` is
/// read, so a `Set`/`Map` pointer (different `obj_type`) or a primitive
/// (`js_nanbox_get_pointer` yields 0) safely resolves to `None`.
pub fn weak_class_id_from_receiver(receiver: f64) -> Option<u32> {
    let addr = js_nanbox_get_pointer(receiver) as usize;
    // #4004: reject the small-handle band (Web Fetch / node:http / timer ids
    // are NaN-boxed POINTER_TAG values, not heap addresses) before
    // dereferencing the GC header. WeakMap/WeakSet are ObjectHeader-backed
    // allocations above the cutoff. See `value::addr_class` for the band map.
    unsafe {
        match crate::value::addr_class::try_read_gc_header(addr) {
            Some(header) if header.obj_type == crate::gc::GC_TYPE_OBJECT => {}
            _ => return None,
        }
        let cid = (*(addr as *const ObjectHeader)).class_id;
        if cid == CLASS_ID_WEAKMAP || cid == CLASS_ID_WEAKSET {
            return Some(cid);
        }
    }
    None
}

unsafe fn entries_array(reg: *mut ObjectHeader) -> *mut ArrayHeader {
    // #6136: `js_string_from_bytes` allocates and can fire a moving minor GC,
    // which relocates the (movable, GcHeader-backed) WeakMap/WeakSet `reg`.
    // Root it across the allocation and re-derive before dereferencing.
    let scope = crate::gc::RuntimeHandleScope::new();
    let reg_handle = scope.root_raw_mut_ptr(reg);
    let entries_key = crate::string::js_string_from_bytes(b"__perry_wk_entries".as_ptr(), 18);
    let reg = reg_handle.get_raw_mut_ptr::<ObjectHeader>();
    let entries_val = js_object_get_field_by_name(reg, entries_key);
    (entries_val.bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ArrayHeader
}

#[no_mangle]
pub extern "C" fn js_weakmap_new() -> *mut ObjectHeader {
    // #1766: sentinel-named slot so `(wm as any).entries` returns
    // `undefined` like Node, instead of leaking the [k, v]-pair array.
    let packed = b"__perry_wk_entries\0";
    let obj = js_object_alloc_with_shape(WEAKMAP_SHAPE_ID, 1, packed.as_ptr(), packed.len() as u32);
    let entries_arr = js_array_alloc(0);
    js_object_set_field(obj, 0, JSValue::array_ptr(entries_arr));
    // Stamp the GC-stable kind marker so dynamic method dispatch
    // (js_native_call_method) recognises this as a WeakMap. Issue #1757.
    unsafe {
        (*obj).class_id = CLASS_ID_WEAKMAP;
    }
    obj
}

/// `WeakMap ( [ iterable ] )`'s `AddEntriesFromIterable` step. `map` is the
/// already-allocated (empty) WeakMap from `js_weakmap_new`; this only
/// populates it. #5834: only fetches/validates the `set` adder when
/// `iterable` is present (spec steps 6-7 — null/undefined return BEFORE
/// `Get(map, "set")`, so a poisoned `WeakMap.prototype.set` getter must not
/// fire for `new WeakMap()` / `new WeakMap(null)`), then drives the iterable
/// with lazy per-item stepping (`iterator_next_value`) so an abrupt
/// `Get`/adder-call closes the iterator (`IteratorClose`) before rethrowing.
/// Mirrors `js_map_from_iterable` (`map.rs`).
#[no_mangle]
pub extern "C" fn js_weakmap_init_iterable(map: f64, iterable: f64) -> f64 {
    use crate::collection_iter::{constructor_iter, ConstructorIter};

    if crate::collection_iter::is_null_or_undefined(iterable) {
        return map;
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let map_handle = scope.root_nanbox_f64(map);
    let iterable_handle = scope.root_nanbox_f64(iterable);

    let adder = crate::collection_iter::require_callable(
        crate::collection_iter::builtin_prototype_adder(
            "WeakMap",
            "set",
            map_handle.get_nanbox_f64(),
        ),
        "WeakMap.prototype.set",
    );
    let adder = crate::collection_iter::normalize_callable_value(adder);
    let adder_handle = scope.root_nanbox_f64(adder);

    fn add_entry(
        map_handle: crate::gc::RuntimeHandle<'_>,
        adder_handle: crate::gc::RuntimeHandle<'_>,
        entry: f64,
        iter_to_close: Option<f64>,
    ) {
        if !crate::collection_iter::is_entry_object(entry) {
            if let Some(iter) = iter_to_close {
                crate::collection_iter::iterator_close(iter);
            }
            crate::collection_iter::throw_not_entry_object(entry);
        }
        let entry_bits = entry.to_bits() as i64;
        let result = crate::collection_iter::call_capturing_throw(|| {
            let key = crate::object::js_object_get_index_polymorphic(entry_bits, 0.0);
            let val = crate::object::js_object_get_index_polymorphic(entry_bits, 1.0);
            let adder = adder_handle.get_nanbox_f64();
            let map = map_handle.get_nanbox_f64();
            crate::collection_iter::call_with_this_capturing_throw(adder, map, &[key, val])
                .unwrap_or_else(|exc| crate::exception::js_throw(exc))
        });
        if let Err(exc) = result {
            if let Some(iter) = iter_to_close {
                crate::collection_iter::iterator_close(iter);
            }
            crate::exception::js_throw(exc);
        }
    }

    match constructor_iter(iterable_handle.get_nanbox_f64()) {
        ConstructorIter::Empty => {}
        ConstructorIter::Array(arr_value) => {
            let arr_handle = scope.root_nanbox_f64(arr_value);
            let arr_ptr = js_nanbox_get_pointer(arr_handle.get_nanbox_f64()) as *mut ArrayHeader;
            if !arr_ptr.is_null() {
                let len = unsafe { js_array_length(arr_ptr) as usize };
                for i in 0..len {
                    let entry = unsafe {
                        let arr = js_nanbox_get_pointer(arr_handle.get_nanbox_f64())
                            as *const ArrayHeader;
                        js_array_get_f64(arr, i as u32)
                    };
                    add_entry(map_handle, adder_handle, entry, None);
                }
            }
        }
        ConstructorIter::Iterator(iter) => {
            let iter_handle = scope.root_nanbox_f64(iter);
            loop {
                let iter = iter_handle.get_nanbox_f64();
                let Some(entry) = crate::collection_iter::iterator_next_value(iter) else {
                    break;
                };
                add_entry(map_handle, adder_handle, entry, Some(iter));
            }
        }
    }

    map_handle.get_nanbox_f64()
}

/// Throw `TypeError: Invalid value used as weak map key` (WeakMap key must be
/// an object). Never returns.
fn throw_invalid_weakmap_key() -> ! {
    let msg = "Invalid value used as weak map key";
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(msg_str);
    crate::exception::js_throw(f64::from_bits(JSValue::pointer(err as *const u8).bits()))
}

/// Throw `TypeError: Invalid value used in weak set` (WeakSet value must be an
/// object). Never returns.
fn throw_invalid_weakset_value() -> ! {
    let msg = "Invalid value used in weak set";
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(msg_str);
    crate::exception::js_throw(f64::from_bits(JSValue::pointer(err as *const u8).bits()))
}

#[no_mangle]
pub extern "C" fn js_weakmap_set(map: f64, key: f64, value: f64) -> f64 {
    // #2772: WeakMap keys must be values that "CanBeHeldWeakly" (ES2023):
    // objects/handles AND non-registered Symbols (a fresh `Symbol()` or a
    // well-known symbol). Only `Symbol.for(...)` registered symbols, and
    // primitives, are invalid. Use `is_valid_weak_target` (shared with
    // WeakRef/FinalizationRegistry) rather than the Map/Set entry-object
    // predicate, which wrongly rejected every Symbol key. Validate at runtime
    // so a value arriving through a variable / dynamic expression still throws
    // (not only the AST-literal fast path in lowering).
    if !is_valid_weak_target(key) {
        throw_invalid_weakmap_key();
    }
    if js_nanbox_get_pointer(map) == 0 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    // #6136: a WeakMap is a movable, GcHeader-backed ObjectHeader (unlike
    // Map/Set, whose plain-alloc headers never move). `weak_entry_new` and
    // `js_array_push_f64` below can fire a moving minor GC that evacuates the
    // WeakMap; a raw `map_ptr` captured before those allocations would dangle,
    // and the new entry would be written into the stale (dead) copy — silently
    // dropping the mapping (the intermittent "wm.get(node) → undefined"
    // symptom). Root map/key/value and re-derive the pointer after every
    // allocating call. Mirrors the #6206 `js_map_set` fix, extended to also
    // root the movable collection object itself.
    let scope = crate::gc::RuntimeHandleScope::new();
    let map_handle = scope.root_nanbox_f64(map);
    let key_handle = scope.root_nanbox_f64(key);
    let value_handle = scope.root_nanbox_f64(value);
    unsafe {
        let map_ptr = js_nanbox_get_pointer(map_handle.get_nanbox_f64()) as *mut ObjectHeader;
        let entries_ptr = entries_array(map_ptr);
        if entries_ptr.is_null() {
            return f64::from_bits(TAG_UNDEFINED);
        }
        let len = js_array_length(entries_ptr) as usize;
        // Update the existing entry if the key matches; remember the first
        // tombstone (an entry whose key the GC collected) so a new key can
        // reuse the freed slot instead of growing the array unboundedly. This
        // scan performs no allocation, so `entries_ptr` stays valid throughout.
        let mut first_tomb: i64 = -1;
        for i in 0..len {
            let entry = weak_entry_at(entries_ptr, i);
            if entry.is_null() {
                continue;
            }
            let stored_key = object_field_bits(entry, WEAK_ENTRY_KEY_FIELD);
            if stored_key == TAG_UNDEFINED {
                if first_tomb < 0 {
                    first_tomb = i as i64;
                }
                continue;
            }
            if stored_key == key_handle.get_nanbox_f64().to_bits() {
                write_object_field_bits_raw(
                    entry,
                    WEAK_ENTRY_VALUE_FIELD,
                    value_handle.get_nanbox_f64().to_bits(),
                );
                return map_handle.get_nanbox_f64();
            }
        }
        // Not present — build a fresh entry. This allocation may move the
        // WeakMap and its entries array, so re-derive both from the rooted
        // handle afterwards. (`js_array_push_f64` internally roots the pushed
        // value across its own grow, so the entry pointer is safe there.)
        let entry = weak_entry_new(key_handle.get_nanbox_f64(), value_handle.get_nanbox_f64());
        // `entries_array` allocates (js_string_from_bytes) and can move the
        // freshly-created `entry` — `weak_holder_register` only records its
        // address, it does not root it. Root the entry across that call and
        // re-read its current pointer before boxing `entry_val`, otherwise a
        // stale address would be stored into the array.
        let entry_handle = scope.root_raw_mut_ptr(entry);
        let map_ptr = js_nanbox_get_pointer(map_handle.get_nanbox_f64()) as *mut ObjectHeader;
        let entries_ptr = entries_array(map_ptr);
        let entry = entry_handle.get_raw_mut_ptr::<ObjectHeader>();
        let entry_val = f64::from_bits(JSValue::pointer(entry as *const u8).bits());
        if first_tomb >= 0 {
            js_array_set_f64(entries_ptr, first_tomb as u32, entry_val);
        } else {
            // js_array_push_f64 may reallocate; rebind the entries field to the
            // (possibly new) header so the append isn't lost. Re-derive map_ptr
            // once more — the push can move the WeakMap object too.
            let grown = js_array_push_f64(entries_ptr, entry_val);
            let map_ptr = js_nanbox_get_pointer(map_handle.get_nanbox_f64()) as *mut ObjectHeader;
            js_object_set_field(map_ptr, 0, JSValue::array_ptr(grown));
        }
    }
    map_handle.get_nanbox_f64()
}

#[no_mangle]
pub extern "C" fn js_weakmap_get(map: f64, key: f64) -> f64 {
    let map_ptr = js_nanbox_get_pointer(map) as *mut ObjectHeader;
    if map_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    unsafe {
        let entries_ptr = entries_array(map_ptr);
        if entries_ptr.is_null() {
            return f64::from_bits(TAG_UNDEFINED);
        }
        let len = js_array_length(entries_ptr) as usize;
        for i in 0..len {
            let entry = weak_entry_at(entries_ptr, i);
            if entry.is_null() {
                continue;
            }
            let stored_key = object_field_bits(entry, WEAK_ENTRY_KEY_FIELD);
            if stored_key == TAG_UNDEFINED {
                continue; // tombstoned (key collected)
            }
            if stored_key == key.to_bits() {
                return f64::from_bits(object_field_bits(entry, WEAK_ENTRY_VALUE_FIELD));
            }
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_weakmap_has(map: f64, key: f64) -> f64 {
    let map_ptr = js_nanbox_get_pointer(map) as *mut ObjectHeader;
    if map_ptr.is_null() {
        return f64::from_bits(TAG_FALSE);
    }
    unsafe {
        let entries_ptr = entries_array(map_ptr);
        if entries_ptr.is_null() {
            return f64::from_bits(TAG_FALSE);
        }
        let len = js_array_length(entries_ptr) as usize;
        for i in 0..len {
            let entry = weak_entry_at(entries_ptr, i);
            if entry.is_null() {
                continue;
            }
            let stored_key = object_field_bits(entry, WEAK_ENTRY_KEY_FIELD);
            if stored_key == TAG_UNDEFINED {
                continue; // tombstoned (key collected)
            }
            if stored_key == key.to_bits() {
                return f64::from_bits(TAG_TRUE);
            }
        }
    }
    f64::from_bits(TAG_FALSE)
}

#[no_mangle]
pub extern "C" fn js_weakmap_delete(map: f64, key: f64) -> f64 {
    if js_nanbox_get_pointer(map) == 0 {
        return f64::from_bits(TAG_FALSE);
    }
    // #6136: root the movable WeakMap, the search key, and the source entries
    // array across the js_array_alloc / js_array_push_f64 allocations below —
    // each can fire a moving minor GC. Without this, the rebuilt array is
    // written into a stale map copy and the loop reads relocated entries from a
    // dangling source pointer (dropping or corrupting entries intermittently).
    let scope = crate::gc::RuntimeHandleScope::new();
    let map_handle = scope.root_nanbox_f64(map);
    let key_handle = scope.root_nanbox_f64(key);
    unsafe {
        let map_ptr = js_nanbox_get_pointer(map_handle.get_nanbox_f64()) as *mut ObjectHeader;
        let entries_ptr = entries_array(map_ptr);
        if entries_ptr.is_null() {
            return f64::from_bits(TAG_FALSE);
        }
        let len = js_array_length(entries_ptr) as usize;
        let entries_handle = scope.root_raw_mut_ptr(entries_ptr);
        let mut found = false;
        // Rebuild without the deleted key AND without tombstones (entries whose
        // key the GC already collected), reclaiming the entry objects.
        let mut new_arr = js_array_alloc(0);
        for i in 0..len {
            // Re-derive the source array each iteration: the previous push may
            // have moved it.
            let entries_ptr = entries_handle.get_raw_mut_ptr::<ArrayHeader>();
            let entry = weak_entry_at(entries_ptr, i);
            if entry.is_null() {
                continue;
            }
            let stored_key = object_field_bits(entry, WEAK_ENTRY_KEY_FIELD);
            if stored_key == TAG_UNDEFINED {
                continue; // drop tombstone
            }
            if stored_key == key_handle.get_nanbox_f64().to_bits() {
                found = true;
                continue;
            }
            let entry_val = f64::from_bits(JSValue::pointer(entry as *const u8).bits());
            new_arr = js_array_push_f64(new_arr, entry_val);
        }
        let map_ptr = js_nanbox_get_pointer(map_handle.get_nanbox_f64()) as *mut ObjectHeader;
        js_object_set_field(map_ptr, 0, JSValue::array_ptr(new_arr));
        if found {
            f64::from_bits(TAG_TRUE)
        } else {
            f64::from_bits(TAG_FALSE)
        }
    }
}

#[no_mangle]
pub extern "C" fn js_weakset_new() -> *mut ObjectHeader {
    // #1766: shares the sentinel name with js_weakmap_new so the same
    // `entries_array` helper reaches the [k,v]-pair storage.
    let packed = b"__perry_wk_entries\0";
    let obj = js_object_alloc_with_shape(WEAKSET_SHAPE_ID, 1, packed.as_ptr(), packed.len() as u32);
    let entries_arr = js_array_alloc(0);
    js_object_set_field(obj, 0, JSValue::array_ptr(entries_arr));
    // Stamp the GC-stable kind marker (see js_weakmap_new). Issue #1757.
    unsafe {
        (*obj).class_id = CLASS_ID_WEAKSET;
    }
    obj
}

/// `WeakSet ( [ iterable ] )`'s iterable-consumption loop. `set` is the
/// already-allocated (empty) WeakSet from `js_weakset_new`; this only
/// populates it. See [`js_weakmap_init_iterable`] for the shared rationale
/// (adder fetched only when `iterable` is present; lazy per-item stepping
/// with `IteratorClose` on an abrupt `add` call). Mirrors `js_set_from_iterable`
/// (`set.rs`).
#[no_mangle]
pub extern "C" fn js_weakset_init_iterable(set: f64, iterable: f64) -> f64 {
    use crate::collection_iter::{constructor_iter, ConstructorIter};

    if crate::collection_iter::is_null_or_undefined(iterable) {
        return set;
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let set_handle = scope.root_nanbox_f64(set);
    let iterable_handle = scope.root_nanbox_f64(iterable);

    let adder = crate::collection_iter::require_callable(
        crate::collection_iter::builtin_prototype_adder(
            "WeakSet",
            "add",
            set_handle.get_nanbox_f64(),
        ),
        "WeakSet.prototype.add",
    );
    let adder = crate::collection_iter::normalize_callable_value(adder);
    let adder_handle = scope.root_nanbox_f64(adder);

    let add_value = |element: f64, iter_to_close: Option<f64>| {
        let adder = adder_handle.get_nanbox_f64();
        let set = set_handle.get_nanbox_f64();
        let result = crate::collection_iter::call_with_this_capturing_throw(adder, set, &[element]);
        if let Err(exc) = result {
            if let Some(iter) = iter_to_close {
                crate::collection_iter::iterator_close(iter);
            }
            crate::exception::js_throw(exc);
        }
    };

    match constructor_iter(iterable_handle.get_nanbox_f64()) {
        ConstructorIter::Empty => {}
        ConstructorIter::Array(arr_value) => {
            let arr_handle = scope.root_nanbox_f64(arr_value);
            let arr_ptr = js_nanbox_get_pointer(arr_handle.get_nanbox_f64()) as *mut ArrayHeader;
            if !arr_ptr.is_null() {
                let len = unsafe { js_array_length(arr_ptr) as usize };
                for i in 0..len {
                    let element = unsafe {
                        let arr = js_nanbox_get_pointer(arr_handle.get_nanbox_f64())
                            as *const ArrayHeader;
                        js_array_get_f64(arr, i as u32)
                    };
                    add_value(element, None);
                }
            }
        }
        ConstructorIter::Iterator(iter) => {
            let iter_handle = scope.root_nanbox_f64(iter);
            loop {
                let iter = iter_handle.get_nanbox_f64();
                let Some(element) = crate::collection_iter::iterator_next_value(iter) else {
                    break;
                };
                add_value(element, Some(iter));
            }
        }
    }

    set_handle.get_nanbox_f64()
}

#[no_mangle]
pub extern "C" fn js_weakset_add(set: f64, value: f64) -> f64 {
    // #2772: WeakSet members must "CanBeHeldWeakly" (ES2023): objects/handles
    // AND non-registered Symbols. Throw the WeakSet-specific message *before*
    // delegating (js_weakmap_set throws the weak-map-key message, which is wrong
    // for a Set). Use `is_valid_weak_target` (not the Map/Set entry-object
    // predicate, which wrongly rejected every Symbol). Validate at runtime so a
    // value arriving through a variable/dynamic expression still throws.
    if !is_valid_weak_target(value) {
        throw_invalid_weakset_value();
    }
    // Store the member as the entry KEY (weak) with an `undefined` value. Using
    // the member as the value too would pin it through the strong value slot and
    // defeat weakness (#2656); a WeakSet only needs key presence, so the value
    // is unused. `has`/`delete` match on the key alone.
    js_weakmap_set(set, value, f64::from_bits(TAG_UNDEFINED));
    set
}

#[no_mangle]
pub extern "C" fn js_weakset_has(set: f64, value: f64) -> f64 {
    js_weakmap_has(set, value)
}

#[no_mangle]
pub extern "C" fn js_weakset_delete(set: f64, value: f64) -> f64 {
    js_weakmap_delete(set, value)
}

/// Throw a `TypeError` for `WeakMap.set(primitive, ...)` / `WeakSet.add(primitive)`.
/// Used by codegen when the static AST key/value is a primitive literal so we can
/// match the JS spec which mandates an exception in those cases.
///
/// Marked `-> f64` for the ABI signature even though `js_throw` is `-> !`;
/// the function never actually returns.
#[no_mangle]
pub extern "C" fn js_weak_throw_primitive() -> f64 {
    let msg = "Invalid value used as weak collection key";
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_error_new_with_message(msg_str);
    let err_val = JSValue::pointer(err as *const u8);
    crate::exception::js_throw(f64::from_bits(err_val.bits()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weak_key_validity_follows_can_be_held_weakly() {
        // ES2023 CanBeHeldWeakly: objects and non-registered symbols may be
        // WeakMap keys / WeakSet members; only primitives and `Symbol.for`
        // (registered) symbols are rejected. Regression guard for the bug where
        // WeakMap/WeakSet used the Map/Set entry-object predicate and wrongly
        // rejected ALL symbol keys with "Invalid value used as weak map key".
        let obj = crate::object::js_object_alloc(0, 0);
        let obj_val = f64::from_bits(JSValue::pointer(obj as *const u8).bits());
        assert!(
            is_valid_weak_target(obj_val),
            "object must be weak-holdable"
        );

        let fresh_sym = unsafe { crate::symbol::js_symbol_new_empty() };
        assert!(
            is_valid_weak_target(fresh_sym),
            "fresh (non-registered) symbol must be weak-holdable"
        );

        let key = "weakkey";
        let key_str = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
        let key_val = f64::from_bits(JSValue::string_ptr(key_str).bits());
        let reg_sym = unsafe { crate::symbol::js_symbol_for(key_val) };
        assert!(
            !is_valid_weak_target(reg_sym),
            "registered Symbol.for symbol must NOT be weak-holdable"
        );

        // Positive round-trip: a fresh symbol key stores and reads back the
        // exact value bits it was given.
        let wm = js_weakmap_new();
        let wm_val = f64::from_bits(JSValue::pointer(wm as *const u8).bits());
        let v = f64::from_bits(JSValue::int32(42).bits());
        js_weakmap_set(wm_val, fresh_sym, v);
        let got = js_weakmap_get(wm_val, fresh_sym);
        assert_eq!(
            got.to_bits(),
            v.to_bits(),
            "symbol-keyed WeakMap entry must round-trip"
        );

        // #5437: a plain number is NOT weak-holdable, even one that lands in the
        // Web Stream id band `[0x100000, 0x200000)`. Only a LIVE registered
        // stream (confirmed by the stdlib `stream_handle_probe`, which is unset
        // in a runtime-only test) is accepted — so a genuine numeric key still
        // throws, matching Node. Guards against blanket-accepting the band.
        assert!(
            !is_valid_weak_target(5.0),
            "plain number must not be weak-holdable"
        );
        assert!(
            !is_valid_weak_target(0x10_0002u64 as f64),
            "stream-band number with no live stream must not be weak-holdable"
        );
    }

    #[test]
    fn weak_collections_inspect_with_items_unknown() {
        // WeakMap/WeakSet contents aren't enumerable, so Node prints the
        // `<items unknown>` placeholder rather than leaking storage fields.
        let wm = js_weakmap_new();
        assert_eq!(
            weak_wrapper_inspect_label(wm),
            Some("WeakMap { <items unknown> }")
        );
        let ws = js_weakset_new();
        assert_eq!(
            weak_wrapper_inspect_label(ws),
            Some("WeakSet { <items unknown> }")
        );
        // WeakRef / FinalizationRegistry have no items placeholder.
        let target = crate::object::js_object_alloc(0, 0);
        let target_val = f64::from_bits(JSValue::pointer(target as *const u8).bits());
        let wr = js_weakref_new(target_val);
        assert_eq!(weak_wrapper_inspect_label(wr), Some("WeakRef {}"));
    }
}
