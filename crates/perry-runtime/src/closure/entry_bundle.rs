//! Argument materialization selected by the callee's static entry. These are
//! GC roots, not an intermediate arguments array or a padded call buffer.

use super::{ClosureHeader, FN_REST_SYNTHETIC_ARGUMENTS, FN_REST_USER_AND_ARGUMENTS};

unsafe fn array_from_cells(values: &[std::cell::UnsafeCell<f64>], arguments: bool) -> f64 {
    let array = crate::array::js_array_alloc_literal(values.len() as u32);
    let elements = crate::array::array_elements_ptr(array).cast::<f64>();
    for (i, value) in values.iter().enumerate() {
        let value = *value.get();
        crate::string::js_string_addref_if_heap_string(value);
        let slot = elements.add(i);
        // GC_STORE_AUDIT(BARRIERED): final array storage, with the same layout
        // note and barrier as js_array_from_values; no per-element push.
        slot.write(value);
        crate::gc::js_gc_note_slot_layout(array as u64, i as u32, value.to_bits());
        crate::gc::js_write_barrier_slot(array as u64, slot as u64, value.to_bits());
    }
    if arguments {
        crate::array::mark_array_as_arguments_object(array);
    }
    crate::value::js_nanbox_pointer(array as i64)
}

/// Called only by the compiler-emitted entry, before it enters the body. The
/// caller buffer is borrowed for this call and never retained. Root values
/// before the first allocation; construct directly into final array storage.
/// Both output values are published together after the last allocation, so a
/// rest-and-arguments body never receives a stale first array.
#[no_mangle]
pub unsafe extern "C" fn js_closure_entry_bundle(
    callee: *const ClosureHeader,
    arguments: *const std::cell::UnsafeCell<f64>,
    count: usize,
    fixed: u32,
    flags: u32,
    rest_out: *mut f64,
    arguments_out: *mut f64,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let callee = scope.root_raw_const_ptr(callee);
    let values = if count == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(arguments, count)
    };
    // The scope owns the root descriptors. Array construction reads the
    // collector-rewritten caller cells directly and needs no handle vector.
    // The existing arguments-object builder consumes explicit handles.
    let handles = if flags & crate::codegen_abi::FN_ARGUMENTS_OBJECT != 0 {
        values
            .iter()
            .map(|cell| scope.root_nanbox_cell(cell))
            .collect()
    } else {
        for cell in values {
            scope.root_nanbox_cell(cell);
        }
        Vec::new()
    };
    let rest = if flags & crate::codegen_abi::FN_ARGUMENTS_OBJECT != 0 {
        crate::value::js_nanbox_pointer(crate::object::arguments_object_alloc_rooted(
            &handles,
            crate::value::js_nanbox_pointer(callee.get_raw_const_ptr::<ClosureHeader>() as i64),
            flags & crate::codegen_abi::FN_ARGUMENTS_RESTRICTED != 0,
            if flags & crate::codegen_abi::FN_ARGUMENTS_MAPPED != 0 {
                fixed
            } else {
                0
            },
        ) as i64)
    } else {
        let start = if flags & FN_REST_SYNTHETIC_ARGUMENTS != 0 {
            0
        } else {
            (fixed as usize).min(count)
        };
        array_from_cells(&values[start..], flags & FN_REST_SYNTHETIC_ARGUMENTS != 0)
    };
    let rest = scope.root_nanbox_f64(rest);
    let all = if flags & FN_REST_USER_AND_ARGUMENTS != 0 {
        array_from_cells(values, true)
    } else {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    };
    rest_out.write(rest.get_nanbox_f64());
    arguments_out.write(all);
}
