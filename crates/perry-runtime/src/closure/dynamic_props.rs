//! Dynamic per-closure property side-table, `this`-rebind/unbind helpers,
//! and the closure-magic-tag pointer predicate.

use super::*;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

static CLOSURE_PROPS: OnceLock<Mutex<HashMap<usize, HashMap<String, f64>>>> = OnceLock::new();

fn get_closure_props() -> &'static Mutex<HashMap<usize, HashMap<String, f64>>> {
    CLOSURE_PROPS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn barrier_closure_dynamic_props(owner: usize, props: &mut HashMap<String, f64>) {
    for value in props.values_mut() {
        crate::gc::runtime_write_barrier_external_slot(
            owner,
            value as *mut f64 as usize,
            value.to_bits(),
        );
    }
}

fn merge_closure_prop_map(
    props: &mut HashMap<usize, HashMap<String, f64>>,
    owner: usize,
    owner_props: HashMap<String, f64>,
) {
    match props.entry(owner) {
        std::collections::hash_map::Entry::Occupied(mut entry) => {
            entry.get_mut().extend(owner_props);
        }
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(owner_props);
        }
    }
}

pub(crate) fn closure_dynamic_props_owner_moved(old_owner: usize, new_owner: usize) {
    if old_owner == 0 || new_owner == 0 || old_owner == new_owner {
        return;
    }
    if let Ok(mut props) = get_closure_props().lock() {
        if let Some(old_props) = props.remove(&old_owner) {
            merge_closure_prop_map(&mut props, new_owner, old_props);
        }
    }
}

pub(crate) fn visit_closure_dynamic_prop_values_mut(owner: usize, mut visit: impl FnMut(&mut f64)) {
    if owner == 0 {
        return;
    }
    let Some(mut owner_props) = get_closure_props()
        .lock()
        .ok()
        .and_then(|mut props| props.remove(&owner))
    else {
        return;
    };

    for value in owner_props.values_mut() {
        visit(value);
    }

    if let Ok(mut props) = get_closure_props().lock() {
        merge_closure_prop_map(&mut props, owner, owner_props);
    }
}

pub(crate) fn visit_closure_dynamic_prop_value_slots_mut(
    owner: usize,
    mut visit: impl FnMut(*mut u64),
) {
    visit_closure_dynamic_prop_values_mut(owner, |value| {
        visit(value as *mut f64 as *mut u64);
    });
}

/// Mutable GC scanner for closure dynamic-property side-table metadata.
///
/// The side table is keyed by closure address, but that key is metadata:
/// it must follow forwarding pointers without itself keeping the closure
/// alive. Property values are traced from `trace_closure`/copied-minor object
/// scanning when their owner closure is live; this scanner handles only
/// post-move key/value fixup and stale-reference verification.
pub fn scan_closure_dynamic_props_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    if !visitor.is_metadata_rewrite_phase() {
        return;
    }
    let mut moved = Vec::new();
    if let Ok(mut props) = get_closure_props().lock() {
        for (&owner, closure_props) in props.iter_mut() {
            let mut new_owner = owner;
            if visitor.visit_metadata_usize_slot(&mut new_owner) {
                moved.push((owner, new_owner));
            }
            for value in closure_props.values_mut() {
                visitor.visit_nanbox_f64_slot(value);
            }
        }
        for (old_owner, new_owner) in moved {
            if let Some(old_props) = props.remove(&old_owner) {
                merge_closure_prop_map(&mut props, new_owner, old_props);
            }
        }
    }
}

/// Check if a raw pointer points to a ClosureHeader by checking CLOSURE_MAGIC at offset 12.
/// Safe to call with any non-null, sufficiently aligned pointer >= 0x10000.
pub fn is_closure_ptr(ptr: usize) -> bool {
    if ptr < 0x10000 {
        return false;
    }
    unsafe {
        let type_tag = *((ptr as *const u8).add(12) as *const u32);
        type_tag == CLOSURE_MAGIC
    }
}

/// Get a dynamic property stored on a closure.
/// Returns TAG_UNDEFINED if not found.
pub fn closure_get_dynamic_prop(ptr: usize, prop: &str) -> f64 {
    if let Ok(props) = get_closure_props().lock() {
        if let Some(closure_props) = props.get(&ptr) {
            if let Some(&val) = closure_props.get(prop) {
                return val;
            }
        }
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Set a dynamic property on a closure.
pub fn closure_set_dynamic_prop(ptr: usize, prop: &str, value: f64) {
    if let Ok(mut props) = get_closure_props().lock() {
        let closure_props = props.entry(ptr).or_insert_with(HashMap::new);
        closure_props.insert(prop.to_string(), value);
        barrier_closure_dynamic_props(ptr, closure_props);
    }
}

/// Snapshot every dynamic property on a closure as `(name, value)` pairs.
/// Sorted alphabetically for stable output (`HashMap` iteration order is
/// non-deterministic). Used by `format_jsvalue` to emit `[Function: f]
/// { ownProp: value }` for functions with user-attached properties. See
/// #1203.
pub fn closure_dynamic_props_snapshot(ptr: usize) -> Vec<(String, f64)> {
    if let Ok(props) = get_closure_props().lock() {
        if let Some(map) = props.get(&ptr) {
            let mut out: Vec<(String, f64)> = map.iter().map(|(k, v)| (k.clone(), *v)).collect();
            out.sort_by(|a, b| a.0.cmp(&b.0));
            return out;
        }
    }
    Vec::new()
}

/// Unbind `this` from a detached method closure.
///
/// When a method is read from an object via PropertyGet (e.g., `const fn = holder.getX`),
/// this function is called on the result. If the value is a closure whose capture_count
/// has CAPTURES_THIS_FLAG set (indicating slot 0 is `this`), it allocates a new closure
/// with the same func_ptr and captures but slot 0 set to undefined.
///
/// For non-closure values (numbers, strings, objects, arrays), this is a no-op.
#[no_mangle]
pub extern "C" fn js_closure_unbind_this(val: f64) -> f64 {
    let bits = val.to_bits();
    let tag = bits & 0xFFFF_0000_0000_0000;
    // Only process POINTER_TAG values (closures are NaN-boxed with POINTER_TAG)
    if tag != 0x7FFD_0000_0000_0000 {
        return val;
    }
    let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
    if ptr < 0x10000 {
        return val;
    }
    // Check CLOSURE_MAGIC
    unsafe {
        let type_tag = *((ptr as *const u8).add(12) as *const u32);
        if type_tag != CLOSURE_MAGIC {
            return val;
        }
        let header = ptr as *const ClosureHeader;
        let raw_count = (*header).capture_count;
        // Only unbind if the closure has the CAPTURES_THIS_FLAG
        if raw_count & CAPTURES_THIS_FLAG == 0 {
            return val;
        }
        let count = real_capture_count(raw_count) as usize;
        if count == 0 {
            return val;
        }
        // Clone the closure with slot 0 set to undefined
        let scope = crate::gc::RuntimeHandleScope::new();
        let val_handle = scope.root_nanbox_f64(val);
        let func_ptr = (*header).func_ptr;
        let new_closure = js_closure_alloc(func_ptr, raw_count);
        let source_bits = val_handle.get_nanbox_f64().to_bits();
        let source_ptr = (source_bits & 0x0000_FFFF_FFFF_FFFF) as usize;
        let source_type_tag =
            std::ptr::read_volatile((source_ptr as *const u8).add(12) as *const u32);
        if source_type_tag != CLOSURE_MAGIC {
            return val_handle.get_nanbox_f64();
        }
        let src_captures = closure_capture_slots_mut(source_ptr as *mut ClosureHeader);
        let dst_captures = closure_capture_slots_mut(new_closure);
        // Set slot 0 to undefined
        *dst_captures = crate::value::TAG_UNDEFINED;
        // Copy remaining captures (slots 1..count)
        for i in 1..count {
            *dst_captures.add(i) = *src_captures.add(i);
        }
        rebuild_closure_layout_and_barriers(new_closure, count);
        // NaN-box the new closure pointer
        let new_ptr = new_closure as u64;
        f64::from_bits(0x7FFD_0000_0000_0000 | (new_ptr & 0x0000_FFFF_FFFF_FFFF))
    }
}

/// Issue #450: clone an accessor closure (from `Object.defineProperty(obj, k, { get, set })`)
/// and patch its reserved `this` slot with `recv_box` (the NaN-boxed target object pointer).
///
/// The user's descriptor object literal's `{ get() {...}, set() {...} }` methods are codegen'd
/// with `captures_this: true` — at object-literal construction the codegen patches their
/// reserved `this` slot to point to the *descriptor* object. But spec says the getter/setter
/// runs with `this === obj` (the property access target, NOT the descriptor). So we clone
/// the closure once at defineProperty time and rebind `this` to `obj`. The original
/// descriptor closure is untouched (in case the user reuses it).
///
/// `closure_bits` is the NaN-boxed closure value (POINTER_TAG | ptr); `recv_box` is the
/// NaN-boxed target receiver (POINTER_TAG | obj). Returns the new closure as NaN-boxed bits,
/// or returns `closure_bits` unchanged if the input isn't a CAPTURES_THIS closure.
///
/// Reserved `this` slot index is `auto_captures.len()` per the codegen convention
/// (`crates/perry-codegen/src/expr.rs::lower_object_literal` and
/// `crates/perry-runtime/src/symbol.rs::js_object_set_symbol_method` — both use the LAST
/// capture slot, i.e. `real_count - 1`, as the `this` slot for `captures_this` closures).
pub(crate) fn clone_closure_rebind_this(closure_bits: u64, recv_box: f64) -> u64 {
    let tag = closure_bits & 0xFFFF_0000_0000_0000;
    if tag != 0x7FFD_0000_0000_0000 {
        return closure_bits;
    }
    let ptr = (closure_bits & 0x0000_FFFF_FFFF_FFFF) as usize;
    if ptr < 0x10000 {
        return closure_bits;
    }
    unsafe {
        let type_tag = std::ptr::read_volatile((ptr as *const u8).add(12) as *const u32);
        if type_tag != CLOSURE_MAGIC {
            return closure_bits;
        }
        let header = ptr as *const ClosureHeader;
        let raw_count = (*header).capture_count;
        // No CAPTURES_THIS_FLAG → the closure body doesn't read `this`, no rebind needed.
        if raw_count & CAPTURES_THIS_FLAG == 0 {
            return closure_bits;
        }
        let count = real_capture_count(raw_count) as usize;
        if count == 0 {
            return closure_bits;
        }
        // Allocate a fresh closure with the same func_ptr + capture_count (preserving the flag).
        let scope = crate::gc::RuntimeHandleScope::new();
        let closure_handle = scope.root_nanbox_u64(closure_bits);
        let recv_handle = scope.root_nanbox_f64(recv_box);
        let func_ptr = (*header).func_ptr;
        let new_closure = js_closure_alloc(func_ptr, raw_count);
        let source_bits = closure_handle.get_nanbox_u64();
        let source_ptr = (source_bits & 0x0000_FFFF_FFFF_FFFF) as usize;
        let source_type_tag =
            std::ptr::read_volatile((source_ptr as *const u8).add(12) as *const u32);
        if source_type_tag != CLOSURE_MAGIC {
            return source_bits;
        }
        let src_captures = closure_capture_slots_mut(source_ptr as *mut ClosureHeader);
        let dst_captures = closure_capture_slots_mut(new_closure);
        // Copy every capture verbatim, then overwrite the `this` slot (last) with recv_box.
        for i in 0..count {
            *dst_captures.add(i) = *src_captures.add(i);
        }
        let this_slot = count - 1;
        *dst_captures.add(this_slot) = recv_handle.get_nanbox_f64().to_bits();
        rebuild_closure_layout_and_barriers(new_closure, count);
        let new_ptr = new_closure as u64;
        0x7FFD_0000_0000_0000 | (new_ptr & 0x0000_FFFF_FFFF_FFFF)
    }
}
