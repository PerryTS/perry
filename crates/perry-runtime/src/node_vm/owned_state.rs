//! VM facts belong to the owner's ordinary property holder. Private GC arrays
//! carry the records; none of their slots is an enumerable user property.
use super::*;
const SOURCE: u32 = 2;

unsafe fn state_bag(owner: usize, create: bool) -> *mut ArrayHeader {
    crate::object::owned_state_bag::bag(owner, create, SOURCE)
}
fn load(owner: usize, slot: u32) -> f64 {
    unsafe {
        let bag = state_bag(owner, false);
        if bag.is_null() {
            undefined_value()
        } else {
            crate::array::js_array_get_f64(bag, slot)
        }
    }
}
fn store(owner: usize, slot: u32, value: f64) {
    unsafe {
        let bag = state_bag(owner, true);
        assert!(
            !bag.is_null(),
            "VM state requires an owning property holder"
        );
        crate::array::js_array_set_f64(bag, slot, value);
    }
}
pub(super) fn function_source(owner: usize) -> Option<String> {
    string_from_value(load(owner, SOURCE))
}
pub(crate) fn set_function_source(owner: usize, source: &str) {
    let _no_move = crate::gc::GcSuppressScope::new();
    store(owner, SOURCE, string_value(source));
}
#[cfg(test)]
pub(crate) fn clear_function_source(owner: usize) {
    store(owner, SOURCE, undefined_value());
}
const SCRIPT: u32 = 1;
unsafe fn record(owner: usize, slot: u32) -> *const ArrayHeader {
    let bits = load(owner, slot).to_bits();
    if bits & crate::value::TAG_MASK == crate::value::POINTER_TAG {
        (bits & crate::value::POINTER_MASK) as *const ArrayHeader
    } else {
        std::ptr::null()
    }
}
pub(super) fn script_metadata(owner: usize) -> Option<ScriptMetadata> {
    unsafe {
        let record = record(owner, SCRIPT);
        if record.is_null() {
            return None;
        }
        Some(ScriptMetadata {
            source: string_from_value(crate::array::js_array_get_f64(record, 0))?,
            filename: string_from_value(crate::array::js_array_get_f64(record, 1))?,
            line_offset: crate::array::js_array_get_f64(record, 2) as i32,
            column_offset: crate::array::js_array_get_f64(record, 3) as i32,
        })
    }
}
pub(super) fn set_script_metadata(owner: usize, metadata: &ScriptMetadata) {
    let _no_move = crate::gc::GcSuppressScope::new();
    let record = crate::array::js_array_alloc_with_length(4);
    crate::array::js_array_set_f64(record, 0, string_value(&metadata.source));
    crate::array::js_array_set_f64(record, 1, string_value(&metadata.filename));
    crate::array::js_array_set_f64(record, 2, metadata.line_offset as f64);
    crate::array::js_array_set_f64(record, 3, metadata.column_offset as f64);
    store(owner, SCRIPT, object_value(record.cast()));
}
#[cfg(test)]
pub(crate) fn script_fields_for_test(owner: usize) -> Option<(String, String, i32, i32)> {
    script_metadata(owner).map(|m| (m.source, m.filename, m.line_offset, m.column_offset))
}
#[cfg(test)]
pub(crate) fn seed_script_for_test(owner: usize) {
    set_script_metadata(
        owner,
        &ScriptMetadata {
            source: "37 + 5".to_owned(),
            filename: "owned-script.ts".to_owned(),
            line_offset: -2,
            column_offset: 3,
        },
    );
}
#[cfg(test)]
pub(crate) fn clear_script_for_test(owner: usize) {
    store(owner, SCRIPT, undefined_value());
}
const CONTEXT: u32 = 0;
pub(super) fn context_state(owner: usize) -> Option<ContextState> {
    unsafe {
        let record = record(owner, CONTEXT);
        if record.is_null() {
            return None;
        }
        Some(ContextState {
            returned_bits: crate::array::js_array_get(record, 0).bits(),
            sandbox_bits: crate::array::js_array_get(record, 1).bits(),
            global_this_bits: crate::array::js_array_get(record, 2).bits(),
            intrinsics_bits: crate::array::js_array_get(record, 3).bits(),
            lexical_env_bits: crate::array::js_array_get(record, 4).bits(),
            strings_allowed: crate::array::js_array_get_f64(record, 5) != 0.0,
            wasm_allowed: crate::array::js_array_get_f64(record, 6) != 0.0,
            microtask_after_evaluate: crate::array::js_array_get_f64(record, 7) != 0.0,
        })
    }
}
pub(super) fn set_context_state(owner: usize, state: &ContextState) {
    let _no_move = crate::gc::GcSuppressScope::new();
    let record = crate::array::js_array_alloc_with_length(8);
    for (index, bits) in [
        state.returned_bits,
        state.sandbox_bits,
        state.global_this_bits,
        state.intrinsics_bits,
        state.lexical_env_bits,
    ]
    .into_iter()
    .enumerate()
    {
        crate::array::js_array_set(record, index as u32, JSValue::from_bits(bits));
    }
    crate::array::js_array_set_f64(record, 5, u32::from(state.strings_allowed) as f64);
    crate::array::js_array_set_f64(record, 6, u32::from(state.wasm_allowed) as f64);
    crate::array::js_array_set_f64(record, 7, u32::from(state.microtask_after_evaluate) as f64);
    store(owner, CONTEXT, object_value(record.cast()));
}
#[cfg(test)]
pub(crate) fn seed_context_for_test(owner: usize, child: u64) {
    let bits = crate::value::POINTER_TAG | owner as u64;
    set_context_state(
        owner,
        &ContextState {
            returned_bits: bits,
            sandbox_bits: bits,
            global_this_bits: bits,
            intrinsics_bits: child,
            lexical_env_bits: child,
            strings_allowed: false,
            wasm_allowed: true,
            microtask_after_evaluate: true,
        },
    );
}
#[cfg(test)]
pub(crate) fn context_fields_for_test(owner: usize) -> Option<(u64, u64, bool, bool, bool)> {
    context_state(owner).map(|s| {
        (
            s.returned_bits,
            s.intrinsics_bits,
            s.strings_allowed,
            s.wasm_allowed,
            s.microtask_after_evaluate,
        )
    })
}
#[cfg(test)]
pub(crate) fn clear_context_for_test(owner: usize) {
    store(owner, CONTEXT, undefined_value());
}
