//! Allocation-free stable-shape arm for compiled `for...in` loops (#8694).

use super::*;

/// The semantic identity of the canonical `%Object.prototype%` chain used by
/// the one-key proof.  Addresses are comparison tokens only, never
/// dereferenced from the cache, so this record is not a GC root.  A moving
/// collection re-derives a different live address and causes a conservative
/// miss; a descriptor, key, or prototype mutation mints a new ShapeId.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PrototypeSignature {
    prototype_addr: usize,
    shape_id: u32,
    vtable_generation: u64,
}

#[derive(Clone, Copy)]
struct PrototypeVerdict {
    signature: PrototypeSignature,
    no_enumerable_chain_keys: bool,
}

crate::perry_thread_local! {
    static PROTOTYPE_VERDICT: std::cell::Cell<Option<PrototypeVerdict>> =
        const { std::cell::Cell::new(None) };
}

fn for_in_diag_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| crate::gc::env_flag_enabled("PERRY_FOR_IN_DIAG"))
}

fn stable_miss<T>(reason: &'static str) -> Option<T> {
    if for_in_diag_enabled() {
        static REPORTED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        if REPORTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 8 {
            eprintln!("FOR-IN-DIAG miss={reason}");
        }
    }
    None
}

unsafe fn prototype_signature(prototype_addr: usize) -> Option<PrototypeSignature> {
    let header = crate::value::addr_class::try_read_gc_header(prototype_addr)?;
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    let prototype = prototype_addr as *const ObjectHeader;
    let shape_id = super::super::shapes::object_shape_id(prototype);
    if shape_id == 0 {
        return None;
    }
    Some(PrototypeSignature {
        prototype_addr,
        shape_id,
        vtable_generation: super::super::class_registry::vtable_generation(),
    })
}

/// Whether the ordinary prototype contributes no enumerable key and itself
/// has no prototype.  The cold recompute walks its authoritative shape keys
/// and descriptor metadata without allocating in Perry's heap; the hot path
/// compares only the prototype's immutable semantic ShapeId.
fn canonical_prototype_has_no_enumerable_chain_keys(receiver_addr: usize) -> bool {
    let prototype_addr = crate::array::object_prototype_addr();
    if prototype_addr == 0
        || prototype_addr == receiver_addr
        || super::super::prototype_chain::object_static_prototype(prototype_addr).is_some()
    {
        return false;
    }
    let Some(now) = (unsafe { prototype_signature(prototype_addr) }) else {
        return false;
    };
    if let Some(verdict) = PROTOTYPE_VERDICT.with(|cell| cell.get()) {
        if verdict.signature == now {
            return verdict.no_enumerable_chain_keys;
        }
    }

    // Cold once per prototype generation.  Built-in Object.prototype methods
    // are physical but non-enumerable, so testing raw key-count would make the
    // optimization permanently vacuous.  Do not call `js_object_keys` here:
    // that allocates and could move the unrooted receiver passed to this native
    // helper before its shape pointer is returned.
    let prototype = prototype_addr as *const ObjectHeader;
    let keys = unsafe { crate::object::object_keys_array(prototype) };
    let key_count = if keys.is_null() {
        0
    } else {
        crate::array::js_array_length(keys)
    };
    let mut no_enumerable_chain_keys = true;
    for index in 0..key_count {
        let key = crate::array::js_array_get(keys, index);
        if !unsafe { super::enumeration::descriptor_marks_non_enumerable(prototype, key) } {
            no_enumerable_chain_keys = false;
            break;
        }
    }
    if no_enumerable_chain_keys {
        no_enumerable_chain_keys = !super::super::accessor_descriptor_keys_for_obj(prototype_addr)
            .iter()
            .any(|key| {
                super::super::get_property_attrs(prototype_addr, key)
                    .is_some_and(|attrs| attrs.enumerable())
            });
    }
    let after_addr = crate::array::object_prototype_addr();
    let after = unsafe { prototype_signature(after_addr) };
    if after == Some(now)
        && super::super::prototype_chain::object_static_prototype(after_addr).is_none()
    {
        PROTOTYPE_VERDICT.with(|cell| {
            cell.set(Some(PrototypeVerdict {
                signature: now,
                no_enumerable_chain_keys,
            }))
        });
    }
    no_enumerable_chain_keys
}

/// Return the receiver's immutable shape-owned one-key snapshot when every
/// JavaScript enumeration input is stable and exact.
fn stable_single_own_for_in_keys(value: f64) -> Option<*mut ArrayHeader> {
    let receiver = JSValue::from_bits(value.to_bits());
    if !receiver.is_pointer() {
        return stable_miss("non_pointer");
    }
    let receiver_addr = (receiver.bits() & crate::value::POINTER_MASK) as usize;
    if !crate::value::addr_class::is_above_handle_band(receiver_addr) {
        return stable_miss("handle_band");
    }
    let Some(receiver_gc) =
        (unsafe { crate::value::addr_class::try_read_gc_header(receiver_addr) })
    else {
        return stable_miss("invalid_header");
    };
    if receiver_gc.obj_type != crate::gc::GC_TYPE_OBJECT
        || receiver_gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || receiver_gc._reserved & crate::gc::OBJ_FLAG_NULL_PROTO != 0
    {
        return stable_miss("receiver_kind");
    }

    let object = receiver_addr as *const ObjectHeader;
    let descriptor = unsafe {
        let class_id = (*object).class_id;
        if class_id != 0 && !super::super::is_anon_shape_class_id(class_id) {
            return stable_miss("receiver_class");
        }
        if !(*object).meta.is_null() {
            return stable_miss("receiver_meta");
        }
        if !object_is_regular(object) {
            return stable_miss("receiver_shape_kind");
        }
        if super::super::prototype_chain::object_static_prototype(receiver_addr).is_some() {
            return stable_miss("receiver_custom_prototype");
        }
        let Some(descriptor) = super::super::shapes::object_shape_descriptor(object) else {
            return stable_miss("missing_shape");
        };
        descriptor
    };
    if descriptor.logical_key_count != 1 || descriptor.keys == 0 {
        return stable_miss("key_count");
    }

    let keys = descriptor.keys as usize as *mut ArrayHeader;
    let Some(keys_gc) = (unsafe { crate::value::addr_class::try_read_gc_header(keys as usize) })
    else {
        return stable_miss("invalid_keys_header");
    };
    if keys_gc.obj_type != crate::gc::GC_TYPE_ARRAY
        || keys_gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || unsafe { crate::array::keys_array_len_capped_to_capacity(keys) } != 1
    {
        return stable_miss("keys_array");
    }

    // Ordinary literal shapes normally cannot carry Perry-private fields, but
    // bind the proof to the actual observable key.  This also excludes virtual
    // WASI state without an address-keyed registry probe on every invocation.
    let key = crate::array::js_array_get(keys, 0);
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(key_bytes) = (unsafe { crate::string::js_string_key_bytes(key, &mut scratch) }) else {
        return stable_miss("non_string_key");
    };
    if unsafe { super::enumeration::descriptor_marks_non_enumerable(object, key) } {
        return stable_miss("non_enumerable_key");
    }
    if super::enumeration::is_internal_runtime_key_bytes(key_bytes)
        || key_bytes.starts_with(b"__wasi")
    {
        return stable_miss("internal_key");
    }
    if !canonical_prototype_has_no_enumerable_chain_keys(receiver_addr) {
        return stable_miss("object_prototype");
    }

    // Freeze this key-list version as the loop snapshot.  Any body addition
    // now forks the receiver to a successor key array rather than growing the
    // active snapshot in place; deletion is filtered by the HIR's per-key `in`
    // recheck.
    unsafe {
        let keys_gc =
            (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
        (*keys_gc).gc_flags |= crate::gc::GC_FLAG_SHAPE_SHARED;
    }
    Some(keys)
}

fn note_for_in_stable_path(hit: bool) {
    if !for_in_diag_enabled() {
        return;
    }
    static CHECKS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    static HITS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let checks = CHECKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    let hits = HITS.fetch_add(hit as u64, std::sync::atomic::Ordering::Relaxed) + hit as u64;
    let fallbacks = checks - hits;
    if checks <= 8 || (hit && hits == 1) || (!hit && fallbacks == 1) || checks % 100_000 == 0 {
        eprintln!(
            "FOR-IN-DIAG checks={} stable_single={} fallback={}",
            checks, hits, fallbacks
        );
    }
}

/// Guarded entry point used by compiled `for...in` loops.
///
/// The generated program names this helper rather than the generic helper so
/// retained LLVM makes the optimization selection auditable.  Every proof
/// miss reaches [`super::enumeration::js_for_in_keys_value`].
#[no_mangle]
pub extern "C" fn js_for_in_keys_stable_value(value: f64) -> *mut ArrayHeader {
    if let Some(keys) = stable_single_own_for_in_keys(value) {
        note_for_in_stable_path(true);
        return keys;
    }
    note_for_in_stable_path(false);
    super::enumeration::js_for_in_keys_value(value)
}
