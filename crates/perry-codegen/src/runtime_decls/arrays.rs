//! Phase B array operations (extracted from runtime_decls.rs).

use super::*;

/// Phase B array operations (number-typed arrays for the first slice).
///
/// All arrays are stored as raw i64 pointers at the runtime level. The
/// codegen NaN-boxes them with `POINTER_TAG` for storage in locals/params,
/// and unboxes back to raw i64 (`bitcast` + `and POINTER_MASK`) before
/// passing to runtime functions.
///
/// - `js_array_alloc(u32) -> *mut ArrayHeader` — allocate with capacity
/// - `js_array_push_f64(arr, value) -> arr*` — push element, may realloc
///   and return a NEW pointer that the caller must use going forward
/// - `js_array_get_f64(arr, index) -> f64` — read typed-number element
/// - `js_array_length(arr) -> u32` — length (u32, sitofp'd to double for
///   our number ABI)
pub fn declare_phase_b_arrays(module: &mut LlModule) {
    module.declare_function("js_array_alloc", I64, &[I32]);
    // Tagged-template `.raw` side-table helpers (per ECMA-262 §13.2.8.3
    // TaggedTemplate Evaluation step 5: `template[Symbol.raw]` returns
    // an array of raw strings).
    module.declare_function("js_tagged_template_register_raw", I64, &[I64, I64]);
    module.declare_function("js_tagged_template_get_or_init", I64, &[I64, I64, I64]);
    module.declare_function("js_template_raw", I64, &[I64]);
    // Convenience alias for `js_array_alloc(0)`; emitted by lower_call's
    // `new Array()` no-arg branch. Issue #432: clang rejected
    // Effect 3.21.2's `internal/fiberRuntime.ts` IR with
    // "use of undefined value '@js_array_create'" because this
    // declaration was missing — the call site at
    // `lower_call/builtin.rs:217` referenced an undeclared symbol.
    module.declare_function("js_array_create", I64, &[]);
    module.declare_function("js_array_constructor_single", I64, &[DOUBLE]);
    // Exact-sized literal allocator — one call + N direct stores replaces
    // alloc + N×push_f64. See `js_array_alloc_literal` in perry-runtime/src/array.rs.
    module.declare_function("js_array_alloc_literal", I64, &[I32]);
    // #5391: build an array literal from a stack buffer of N values in one call
    // (outlines the inline alloc + per-element store/note/barrier). (values_ptr, n).
    module.declare_function("js_array_from_values", I64, &[PTR, I32]);
    module.declare_function("js_array_push_f64", I64, &[I64, DOUBLE]);
    module.declare_function("js_array_push_guard", VOID, &[I64]);
    module.declare_function("js_array_push_hole", I64, &[I64]);
    module.declare_function("js_array_numeric_push_f64_unboxed", I64, &[I64, DOUBLE]);
    // Refs #488: bulk push for `arr.push(...src)` spread call.
    module.declare_function("js_array_push_spread_f64", I64, &[I64, I64]);
    module.declare_function("js_array_get_f64", DOUBLE, &[I64, I32]);
    module.declare_function("js_array_get_index_or_string", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_array_numeric_get_f64_unboxed", DOUBLE, &[I64, I32]);
    module.declare_function("js_array_set_f64", VOID, &[I64, I32, DOUBLE]);
    module.declare_function("js_array_numeric_set_f64_unboxed", I32, &[I64, I32, DOUBLE]);
    // Extending variant: returns a possibly-realloc'd pointer that the
    // caller must write back to the local slot.
    module.declare_function("js_array_set_f64_extend", I64, &[I64, I32, DOUBLE]);
    module.declare_function("js_array_set_f64_extend_strict", I64, &[I64, I32, DOUBLE]);
    module.declare_function("js_array_fill_f64_const_extend", I64, &[I64, I32, DOUBLE]);
    module.declare_function("js_array_fill_f64_iota_extend", I64, &[I64, I32]);
    module.declare_function("js_array_fill_f64_const_len_extend", I64, &[I64, DOUBLE]);
    module.declare_function("js_array_fill_f64_iota_len_extend", I64, &[I64]);
    module.declare_function("js_array_set_string_key", I64, &[I64, I64, DOUBLE]);
    module.declare_function("js_array_set_index_or_string", I64, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_array_mark_arguments_object", I64, &[I64]);
    module.declare_function("js_array_mark_numeric_f64_layout", I32, &[I64]);
    module.declare_function("js_array_is_numeric_f64_layout", I32, &[I64]);
    module.declare_function("js_array_clear_numeric_layout", VOID, &[I64]);
    module.declare_function("js_array_numeric_value_to_raw_f64", DOUBLE, &[DOUBLE]);
    module.declare_function("js_array_note_numeric_write", VOID, &[I64, I64]);
    module.declare_function("js_array_length", I32, &[I64]);
    // Array.isArray runtime dispatch for values with indeterminate
    // static type (e.g. JSON.parse results, closure captures, any/
    // unknown-typed locals). Returns NaN-boxed boolean.
    module.declare_function("js_array_is_array", DOUBLE, &[DOUBLE]);
    // Issue #73: safe `.length` dispatch by runtime type. Fallback
    // for the inline PropertyGet length path when the GC-type check
    // can't prove the receiver is an Array/String.
    module.declare_function("js_value_length_f64", DOUBLE, &[DOUBLE]);

    // Shadow stack for precise root tracking (gen-GC Phase A per
    // docs/generational-gc-plan.md). Declared now so codegen can
    // reference them; emission at function entry/exit + safepoints
    // is the next milestone.
    //   js_shadow_frame_push(slot_count: u32) -> u64 (frame handle)
    //   js_shadow_frame_pop(frame_handle: u64)
    //   js_shadow_slot_set(idx: u32, value: u64)
    //   js_shadow_slot_bind(idx: u32, value_slot: *mut u64)
    module.declare_function("js_shadow_frame_push", I64, &[I32]);
    module.declare_function("js_shadow_frame_pop", VOID, &[I64]);
    module.declare_function("js_shadow_slot_set", VOID, &[I32, I64]);
    module.declare_function("js_shadow_slot_bind", VOID, &[I32, PTR]);
    module.declare_function("js_gc_write_barriers_emitted", VOID, &[I32]);

    // Write barrier for the generational GC (Phase C per the
    // gen-GC plan). Called by codegen-emitted heap-store sites
    // when sub-phase C2 wires the emission. Records old→young
    // pointer stores in the per-thread remembered set so minor
    // GC can scan precise roots + RS instead of the full old-gen.
    //   js_write_barrier(parent_bits: u64, child_bits: u64)
    //   js_write_barrier_slot(parent_bits: u64, slot_addr: u64, child_bits: u64)
    //   js_write_barrier_root_nanbox(child_bits: u64)
    //   js_write_barrier_root_heap_word(child_bits: u64)
    //   js_gc_note_slot_layout(parent_bits: u64, slot_index: u32, value_bits: u64)
    //   js_gc_init_typed_shape_layout(obj: u64, slot_count: u32, raw_f64_mask_words: *const u64, raw_f64_mask_word_count: u32, pointer_mask_words: *const u64, pointer_mask_word_count: u32)
    //   js_gc_init_unboxed_object_layout(obj: u64, slot_count: u32, raw_f64_mask: u64, pointer_mask: u64)
    module.declare_function("js_write_barrier", VOID, &[I64, I64]);
    module.declare_function("js_write_barrier_slot", VOID, &[I64, I64, I64]);
    module.declare_function("js_write_barrier_root_nanbox", VOID, &[I64]);
    module.declare_function("js_write_barrier_root_heap_word", VOID, &[I64]);
    module.declare_function("js_gc_note_slot_layout", VOID, &[I64, I32, I64]);
    //   js_gc_note_slot_layout_aware(parent, slot_index, value_bits, old_bits)
    module.declare_function("js_gc_note_slot_layout_aware", VOID, &[I64, I32, I64, I64]);
    module.declare_function(
        "js_gc_init_typed_shape_layout",
        VOID,
        &[I64, I32, PTR, I32, PTR, I32],
    );
    module.declare_function(
        "js_gc_init_unboxed_object_layout",
        VOID,
        &[I64, I32, I64, I64],
    );

    // Array methods (Phase B.12).
    // - js_array_pop_f64(arr) -> f64    (last element, NaN if empty)
    // - js_array_join(arr, sep) -> *mut StringHeader (i64)
    // - js_array_join_value(arr, sep_value) -> *mut StringHeader (i64)
    module.declare_function("js_array_pop_f64", DOUBLE, &[I64]);
    module.declare_function("js_array_join", I64, &[I64, I64]);
    module.declare_function("js_array_join_value", I64, &[I64, DOUBLE]);
    module.declare_function("js_array_forEach", VOID, &[I64, I64]);
    module.declare_function("js_array_fill", I64, &[I64, DOUBLE]);
    module.declare_function("js_array_fill_range", I64, &[I64, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function(
        "js_array_fill_generic",
        DOUBLE,
        &[DOUBLE, DOUBLE, I32, DOUBLE, I32, DOUBLE],
    );
    module.declare_function("js_array_delete", I32, &[I64, I32]);
    // Closes #304: `arr.length = N` truncate / extend.
    module.declare_function("js_array_set_length", VOID, &[I64, DOUBLE]);
    module.declare_function("js_array_set_length_strict", VOID, &[I64, DOUBLE]);
    // Array.from() — js_array_clone handles arrays, Sets, and Maps.
    module.declare_function("js_array_clone", I64, &[I64]);
    // #2773: Array.from(source) — throws TypeError for nullish sources, keeps
    // number/boolean/symbol -> [], otherwise materializes via js_array_clone.
    // Takes the raw NaN-boxed value so the tag bits survive.
    module.declare_function("js_array_from_value", I64, &[DOUBLE]);
    // Array.prototype generic receiver materialization — like LengthOfArrayLike,
    // but absent indexed keys remain holes rather than present undefined slots.
    module.declare_function("js_array_from_arraylike_holey_value", I64, &[DOUBLE]);
    // #2874: Iterator.from(x) — wrap any iterable in a lazy iterator-helper
    // object. Returns an already NaN-boxed pointer (DOUBLE).
    module.declare_function("js_iterator_from", DOUBLE, &[DOUBLE]);
    // #2773: Array.from(source, mapFn, thisArg?) — nullish-throw + mapFn
    // callability validation + (value,index) mapped call with thisArg binding.
    module.declare_function("js_array_from_mapped", I64, &[DOUBLE, DOUBLE, DOUBLE]);
    // #2805: Array.prototype.concat(...args) — non-mutating, variadic, with
    // Symbol.isConcatSpreadable handling. (recv_handle, args_ptr, count).
    module.declare_function("js_array_concat_variadic", I64, &[I64, PTR, I32]);
    // #4597: generic `Array.prototype.<m>.call/apply(arrayLike, …)` — operate on
    // the original receiver value (ToObject + LengthOfArrayLike + indexed
    // Get/HasProperty). All take/return NaN-boxed DOUBLE values.
    for f in [
        "js_arraylike_forEach",
        "js_arraylike_map",
        "js_arraylike_filter",
        "js_arraylike_some",
        "js_arraylike_every",
        "js_arraylike_find",
        "js_arraylike_findIndex",
        "js_arraylike_findLast",
        "js_arraylike_findLastIndex",
    ] {
        module.declare_function(f, DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    }
    module.declare_function(
        "js_arraylike_reduce",
        DOUBLE,
        &[DOUBLE, DOUBLE, I32, DOUBLE],
    );
    module.declare_function(
        "js_arraylike_reduceRight",
        DOUBLE,
        &[DOUBLE, DOUBLE, I32, DOUBLE],
    );
    module.declare_function(
        "js_arraylike_indexOf",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, I32],
    );
    module.declare_function(
        "js_arraylike_lastIndexOf",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, I32],
    );
    module.declare_function(
        "js_arraylike_includes",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, I32],
    );
    module.declare_function("js_arraylike_at", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_arraylike_join", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function(
        "js_arraylike_slice",
        DOUBLE,
        &[DOUBLE, DOUBLE, I32, DOUBLE, I32],
    );
    module.declare_function("js_arraylike_sort", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_arraylike_splice", DOUBLE, &[DOUBLE, PTR, I32]);
    module.declare_function("js_arraylike_concat", DOUBLE, &[DOUBLE, PTR, I32]);
    // Generic mutators over a value receiver (`Array.prototype.{pop,shift,
    // push,unshift}.call/apply(recv, …)`) — primitive / array-like receivers.
    module.declare_function("js_arraylike_pop", DOUBLE, &[DOUBLE]);
    module.declare_function("js_arraylike_shift", DOUBLE, &[DOUBLE]);
    module.declare_function("js_arraylike_push", DOUBLE, &[DOUBLE, PTR, I32]);
    module.declare_function("js_arraylike_unshift", DOUBLE, &[DOUBLE, PTR, I32]);
    // Spread `[...x]` — strict GetIterator/materialization.
    module.declare_function("js_array_clone_for_spread", I64, &[DOUBLE]);
    module.declare_function("js_array_spread_append", I64, &[I64, DOUBLE]);
    // Generator / iterator protocol: walk `.next()`/`.value` loop and collect into array.
    module.declare_function("js_iterator_to_array", I64, &[DOUBLE]);
    module.declare_function("js_iterator_next_result", DOUBLE, &[DOUBLE]);
    module.declare_function("js_iterator_close_if_not_done", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_iterator_rest_to_array", DOUBLE, &[DOUBLE, DOUBLE]);
    // #1831: `yield*` iterator resolution — `operand[Symbol.iterator]()` or the
    // operand itself when already an iterator. Returns a NaN-boxed JSValue.
    module.declare_function("js_get_iterator", DOUBLE, &[DOUBLE]);
    module.declare_function("js_get_async_iterator", DOUBLE, &[DOUBLE]);
    // #321: materialize an untyped `for...of` receiver into a plain Array by
    // inspecting its runtime GC kind (Map/Set/Array/string/iterable).
    // Returns a NaN-boxed array JSValue.
    module.declare_function("js_for_of_to_array", DOUBLE, &[DOUBLE]);

    declare_phase_b_objects(module);
}
