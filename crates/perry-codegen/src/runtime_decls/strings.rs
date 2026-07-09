//! Phase B string operations (extracted from runtime_decls.rs).

use super::*;

/// Phase B string operations.
///
/// `js_string_concat(*const StringHeader, *const StringHeader) -> *mut StringHeader`
/// — both arguments and the return value are raw i64 pointers in our ABI
/// (no NaN-tag). The codegen unboxes the operands by `bitcast double → i64`
/// and `and` with `POINTER_MASK` (0x0000_FFFF_FFFF_FFFF), then re-boxes the
/// result with `js_nanbox_string`.
pub fn declare_phase_b_strings(module: &mut LlModule) {
    module.declare_function("js_string_concat", I64, &[I64, I64]);
    // SSO-aware concat: NaN-boxed f64 in, NaN-boxed f64 out. Avoids
    // the `js_get_string_pointer_unified`-driven SSO materialization
    // that the legacy `js_string_concat` ABI forced on every concat
    // with at least one SSO operand. For the SSO+SSO=SSO sub-case
    // also avoids the result heap allocation.
    module.declare_function("js_string_concat_box", DOUBLE, &[DOUBLE, DOUBLE]);
    // Dynamic string coercion: takes any NaN-boxed JSValue and returns a
    // raw string handle (`crates/perry-runtime/src/value.rs:813`).
    module.declare_function("js_jsvalue_to_string", I64, &[DOUBLE]);
    // #3146: nullish-guarded `.toString()` member-call variant.
    module.declare_function("js_jsvalue_to_string_method", I64, &[DOUBLE]);
    // ToString coercion (undefined→"undefined", null→"null", objects dispatch
    // toString) — used by RegExp exec/test arg + constructor coercion.
    module.declare_function("js_jsvalue_to_string_coerce", I64, &[DOUBLE]);

    // Fused string+value concat (issue #58): collapses js_jsvalue_to_string +
    // js_string_concat into a single allocation for number operands.
    // `js_string_concat_value(prefix_handle, value_f64) -> handle`
    // `js_value_concat_string(value_f64, suffix_handle) -> handle`
    module.declare_function("js_string_concat_value", I64, &[I64, DOUBLE]);
    module.declare_function("js_value_concat_string", I64, &[DOUBLE, I64]);

    // N-way string concat (v0.5.769): collapses a left-spine of pairwise
    // string-typed Add nodes (`a + b + c + ...`) into a single allocation.
    // First arg is a stack-allocated array of N NaN-boxed `f64` values;
    // second arg is the count. Returns a raw string handle.
    // (`crates/perry-runtime/src/string.rs::js_string_concat_chain`)
    module.declare_function("js_string_concat_chain", I64, &[I64, I32]);

    // In-place append for the `x = x + y` pattern. When `x` has
    // refcount=1 (unique owner), the runtime mutates in-place and
    // returns the same pointer; otherwise it allocates a new string.
    // Either way the caller must use the returned pointer.
    // (`crates/perry-runtime/src/string.rs:88`)
    module.declare_function("js_string_append", I64, &[I64, I64]);

    // String methods (Phase B.12).
    // All take/return raw i64 string handles. Length args are i32.
    // - js_string_index_of(haystack, needle) -> i32
    // - js_string_index_of_from(haystack, needle, from) -> i32
    // - js_string_slice(s, start, end) -> *mut StringHeader (i64)
    // - js_string_substring(s, start, end) -> *mut StringHeader (i64)
    // - js_string_starts_with(s, prefix) -> i32 (boolean as 0/1)
    // - js_string_ends_with(s, suffix) -> i32
    module.declare_function("js_string_index_of", I32, &[I64, I64]);
    module.declare_function("js_string_index_of_from", I32, &[I64, I64, I32]);
    // #2812: ToIntegerOrInfinity for String.includes(search, position).
    module.declare_function("js_string_position_to_index", I32, &[DOUBLE]);
    module.declare_function("js_string_slice", I64, &[I64, I32, I32]);
    module.declare_function("js_string_substring", I64, &[I64, I32, I32]);
    // Legacy substr(start, length); raw NaN-boxed JS values, ToIntegerOrInfinity
    // applied in the runtime helper. An `undefined` length means "rest" (#2897).
    module.declare_function("js_string_substr", I64, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_string_split", I64, &[I64, I64]);
    module.declare_function("js_string_split_n", I64, &[I64, I64, I32]);
    // Boxed separator + boxed limit; full ToUint32(limit)/ToString(separator)
    // coercion + undefined/RegExp handling (ECMA-262 §22.1.3.21).
    module.declare_function("js_string_split_value", I64, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_math_trunc", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_round", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_sign", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_imul", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_math_pow", DOUBLE, &[DOUBLE, DOUBLE]);

    // Math.* unary functions: use LLVM intrinsics directly so we
    // get hardware instructions / libm calls instead of depending
    // on `js_math_*` runtime symbols (which the auto-optimize
    // dead-strip removes from libperry_runtime.a).
    module.declare_function("llvm.sqrt.f64", DOUBLE, &[DOUBLE]);
    module.declare_function("llvm.floor.f64", DOUBLE, &[DOUBLE]);
    module.declare_function("llvm.ceil.f64", DOUBLE, &[DOUBLE]);
    module.declare_function("llvm.fabs.f64", DOUBLE, &[DOUBLE]);
    module.declare_function("llvm.copysign.f64", DOUBLE, &[DOUBLE, DOUBLE]);
    // `llvm.assume` — used by Buffer index-set/get fast paths
    // (`crates/perry-codegen/src/expr.rs::Expr::BufferIndexSet/Get` etc.)
    // and the Buffer numeric-read intrinsics
    // (`lower_call.rs::lower_buffer_numeric_read`) for branchless bounds
    // checks. Apple Clang ≥21 (Xcode 26) auto-recognises the intrinsic
    // even when undeclared in the IR, but Apple Clang 15 (LLVM 17 — what
    // ships on the macOS-14 GitHub runner via Xcode 15.x) errors with
    // `error: use of undefined value '@llvm.assume'`. This was the actual
    // root cause of the long-tail of `ci-env` Buffer/typed-array test
    // skips in `test-parity/known_failures.json` — diagnosed via the
    // compile-stderr capture artifact added in the previous commit.
    module.declare_function("llvm.assume", VOID, &[I1]);
    // `llvm.bswap.i{16,32,64}` — used by Buffer numeric BE-read/write
    // intrinsics (`lower_call.rs::lower_buffer_numeric_read/write` —
    // see the size-keyed lookup table at lower_call.rs:168). Same
    // Apple-Clang-version skew as llvm.assume above: ≥21 (Xcode 26)
    // auto-recognises bswap intrinsics even when undeclared, but
    // Apple Clang 15 (LLVM 17 — macos-14 GitHub runner via Xcode 15.x)
    // errors with `error: use of undefined value '@llvm.bswap.i16'`.
    // Surfaced when the parity job's compile-smoke step started running
    // the un-skipped Buffer family after #241's known_failures.json
    // cleanup.
    module.declare_function("llvm.bswap.i16", I16, &[I16]);
    module.declare_function("llvm.bswap.i32", I32, &[I32]);
    module.declare_function("llvm.bswap.i64", I64, &[I64]);
    module.declare_function("llvm.memset.p0.i64", VOID, &[PTR, I8, I64, I1]);
    module.declare_function("llvm.memmove.p0.p0.i64", VOID, &[PTR, PTR, I64, I1]);
    // Keep js_math_pow for now — Math.pow has overflow / NaN
    // semantics that the libm pow doesn't quite match.

    // JSON.stringify (Phase B.15). The 2-arg form is JsonStringifyFull
    // in the HIR (value, type_hint, indent — actually 3 args; we use the
    // simple 2-arg js_json_stringify for now).
    module.declare_function("js_json_stringify", I64, &[DOUBLE, I32]);

    // Map (Phase B.15). The runtime stores keys/values as NaN-boxed doubles.
    // js_map_alloc returns a *mut MapHeader (i64 pointer).
    module.declare_function("js_map_alloc", I64, &[I32]);
    module.declare_function("js_text_encoder_encode_into_llvm", I64, &[DOUBLE, DOUBLE]);
    // typeof: returns a string handle ("number"/"string"/"boolean"/"undefined"/"object"/"function")
    module.declare_function("js_value_typeof", I64, &[DOUBLE]);
    module.declare_function("js_string_starts_with", I32, &[I64, I64]);
    module.declare_function("js_string_ends_with", I32, &[I64, I64]);
    module.declare_function("js_string_search_value_to_string", I64, &[DOUBLE, I32]);
    // 2-arg form: (s, prefix/suffix, position). Mirrors the spec
    // `String.prototype.startsWith/endsWith(searchString, position)` with
    // UTF-16 code-unit indexing.
    module.declare_function("js_string_starts_with_at", I32, &[I64, I64, I32]);
    module.declare_function("js_string_ends_with_at", I32, &[I64, I64, I32]);

    // Closure / function-as-value primitives (Phase D).
    //
    // - js_closure_alloc(func_ptr, capture_count) -> *mut ClosureHeader
    //     Allocates a closure object pointing at the given function with
    //     space for `capture_count` captured-value slots.
    // - js_closure_set/get_capture_bits(closure, idx, bits)
    //     Read/write a captured value's raw JSValueBits at slot `idx`.
    // - js_closure_set/get_capture_f64(closure, idx, value)
    //     Compatibility shims over the bits helpers for legacy f64 call sites.
    // - js_closure_call0..call16(closure, args…) -> double
    //     Invoke the closure with N args. The runtime extracts the
    //     function pointer from the closure header and calls it with
    //     the closure as the first argument followed by the user args.
    //     The runtime exports js_closure_call0 through js_closure_call16
    //     (see crates/perry-runtime/src/closure.rs); the call site cap in
    //     lower_call.rs matches.
    module.declare_function("js_closure_alloc", I64, &[PTR, I32]);
    // Singleton-cached variant for non-capturing closures and FuncRef
    // wrappers — same `func_ptr` returns the same cached ClosureHeader,
    // skipping per-evaluation closure allocation on the hot loop. See
    // `crates/perry-runtime/src/closure.rs::js_closure_alloc_singleton`.
    module.declare_function("js_closure_alloc_singleton", I64, &[PTR]);
    // Singleton-cached variant for closures with captures, keyed by
    // `(func_ptr, capture_bits…)`. Args: (func_ptr, capture_count,
    // captures_ptr — pointer to `capture_count` u64 values).
    module.declare_function(
        "js_closure_alloc_with_captures_singleton",
        I64,
        &[PTR, I32, PTR],
    );
    module.declare_function("js_closure_set_capture_bits", VOID, &[I64, I32, I64]);
    module.declare_function("js_closure_get_capture_bits", I64, &[I64, I32]);
    module.declare_function("js_closure_set_capture_f64", VOID, &[I64, I32, DOUBLE]);
    module.declare_function("js_closure_get_capture_f64", DOUBLE, &[I64, I32]);
    // Issue #493: register a closure body's rest-param arity in the runtime
    // side-table so `js_closure_callN` can bundle trailing args at call
    // sites where codegen doesn't know the closure's arity statically
    // (e.g. `obj.cb(a, b, c)` where `cb` is a class field holding an
    // arrow with `...rest`). Called once per closure body at module init.
    module.declare_function("js_register_closure_rest", VOID, &[PTR, I32]);
    // Refs #915 (gap 1 from #899): variant that flags the rest param as the
    // HIR-synthesized `arguments` array — the dispatcher then bundles ALL
    // passed args into the rest slot, matching JS spec semantics for
    // `arguments.length` in a `function(a, b) { …arguments… }` returned from
    // another function.
    module.declare_function("js_register_closure_synthetic_arguments", VOID, &[PTR, I32]);
    module.declare_function("js_register_closure_rest_and_arguments", VOID, &[PTR, I32]);
    module.declare_function("js_register_closure_arity", VOID, &[PTR, I32]);
    module.declare_function("js_register_closure_length", VOID, &[PTR, I32]);
    module.declare_function("js_register_closure_arrow_function", VOID, &[PTR]);
    module.declare_function("js_register_closure_strict_function", VOID, &[PTR]);
    module.declare_function("js_register_closure_async_function", VOID, &[PTR]);
    module.declare_function("js_register_closure_generator_function", VOID, &[PTR]);
    module.declare_function("js_register_closure_async_generator_function", VOID, &[PTR]);
    // #5504: tag-checking unbox for dynamic call callees — throws
    // `TypeError: value is not a function` instead of masking a
    // non-pointer value's low 48 bits into a wild closure pointer.
    module.declare_function("js_closure_unbox_callee_checked", I64, &[DOUBLE]);
    module.declare_function("js_closure_call0", DOUBLE, &[I64]);
    module.declare_function("js_closure_call1", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_closure_call2", DOUBLE, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_closure_call3", DOUBLE, &[I64, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function(
        "js_closure_call4",
        DOUBLE,
        &[I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_closure_call5",
        DOUBLE,
        &[I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_closure_call6",
        DOUBLE,
        &[I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_closure_call7",
        DOUBLE,
        &[I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_closure_call8",
        DOUBLE,
        &[
            I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE,
        ],
    );
    module.declare_function(
        "js_closure_call9",
        DOUBLE,
        &[
            I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE,
        ],
    );
    module.declare_function(
        "js_closure_call10",
        DOUBLE,
        &[
            I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE,
        ],
    );
    module.declare_function(
        "js_closure_call11",
        DOUBLE,
        &[
            I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE,
            DOUBLE,
        ],
    );
    module.declare_function(
        "js_closure_call12",
        DOUBLE,
        &[
            I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE,
            DOUBLE, DOUBLE,
        ],
    );
    module.declare_function(
        "js_closure_call13",
        DOUBLE,
        &[
            I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE,
            DOUBLE, DOUBLE, DOUBLE,
        ],
    );
    module.declare_function(
        "js_closure_call14",
        DOUBLE,
        &[
            I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE,
            DOUBLE, DOUBLE, DOUBLE, DOUBLE,
        ],
    );
    module.declare_function(
        "js_closure_call15",
        DOUBLE,
        &[
            I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE,
            DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE,
        ],
    );
    module.declare_function(
        "js_closure_call16",
        DOUBLE,
        &[
            I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE,
            DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE,
        ],
    );

    // Phase B.16 / D follow-ups: more runtime functions discovered
    // by the test-files sweep histogram.
    module.declare_function("js_array_map", I64, &[I64, I64]);
    module.declare_function("js_array_map_discard", VOID, &[I64, I64]);
    module.declare_function("js_array_filter", I64, &[I64, I64]);
    module.declare_function("js_array_concat", I64, &[I64, I64]);
    module.declare_function("js_array_concat_new", I64, &[I64, I64]);
    module.declare_function("js_error_new", I64, &[]);
    module.declare_function("js_error_new_with_message", I64, &[I64]);
    module.declare_function("js_error_new_from_value", I64, &[DOUBLE]);
    module.declare_function("js_error_new_kind_from_value", I64, &[I32, DOUBLE]);
    module.declare_function("js_error_new_with_cause_from_value", I64, &[DOUBLE, DOUBLE]);
    module.declare_function(
        "js_error_new_kind_with_options_from_value",
        I64,
        &[I32, DOUBLE, DOUBLE],
    );
    // `new assert.AssertionError({...})` — Expr::NewDynamic special-case.
    module.declare_function("js_assert_assertion_error_ctor", DOUBLE, &[DOUBLE]);
    // `new assert.Assert(options)` — Expr::NewDynamic special-case.
    module.declare_function("js_assert_assert_ctor", DOUBLE, &[DOUBLE]);
    // Issue #462: thrown by PropertyGet codegen on undefined/null receiver.
    // Helper diverges (`-> !`); declared as void-return for LLVM purposes.
    module.declare_function(
        "js_throw_type_error_property_access",
        VOID,
        &[I32, PTR, I64],
    );
    // Issue #510: thrown by `lower_string_method`'s unknown-method
    // catch-all for primitive (string-typed) receivers. Args:
    // (kind_ptr, kind_len, prop_ptr, prop_len). Helper diverges
    // (`-> !`); declared as void-return for LLVM purposes.
    module.declare_function(
        "js_throw_type_error_not_a_function",
        VOID,
        &[PTR, I64, PTR, I64],
    );
    // Generic "throw Error/TypeError/RangeError with optional Node `.code`".
    // Args: (msg_ptr, msg_len, code_ptr, code_len, kind). Used by the
    // WorkerNew unresolved-path fallback. Helper diverges (`-> !`); declared
    // as void-return for LLVM purposes.
    module.declare_function("js_throw_error_with_code", VOID, &[PTR, I64, PTR, I64, I32]);
    module.declare_function("js_map_set", I64, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_map_set_string_number", I64, &[I64, I64, DOUBLE]);
    module.declare_function("js_map_set_string_key", I64, &[I64, I64, DOUBLE]);
    module.declare_function("js_map_set_string_i32", I64, &[I64, I64, I32]);
    module.declare_function("js_map_set_string_u32", I64, &[I64, I64, I32]);
    module.declare_function("js_map_set_string_f32", I64, &[I64, I64, F32]);
    module.declare_function("js_map_set_string_bool", I64, &[I64, I64, I32]);
    module.declare_function("js_map_set_string_string", I64, &[I64, I64, I64]);
    module.declare_function("js_map_set_number_key", I64, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_map_get", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_map_get_string_key", DOUBLE, &[I64, I64]);
    module.declare_function("js_map_get_number_key", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_map_has", I32, &[I64, DOUBLE]);
    module.declare_function("js_map_has_string_key", I32, &[I64, I64]);
    module.declare_function("js_map_has_number_key", I32, &[I64, DOUBLE]);
    module.declare_function("js_map_delete", I32, &[I64, DOUBLE]);
    module.declare_function("js_map_delete_string_key", I32, &[I64, I64]);
    module.declare_function("js_map_delete_number_key", I32, &[I64, DOUBLE]);
    module.declare_function("js_object_keys", I64, &[I64]);
    module.declare_function("js_object_keys_value", I64, &[DOUBLE]);
    module.declare_function("js_for_in_keys_value", I64, &[DOUBLE]);
    module.declare_function("js_is_finite", DOUBLE, &[DOUBLE]);
    module.declare_function("js_is_undefined_or_bare_nan", I32, &[DOUBLE]);
    module.declare_function("js_math_min_array", DOUBLE, &[I64]);
    module.declare_function("js_math_max_array", DOUBLE, &[I64]);
    module.declare_function("js_math_min2", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_math_max2", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_string_coerce", I64, &[DOUBLE]);
    module.declare_function("js_array_slice", I64, &[I64, I32, I32]);
    module.declare_function("js_array_slice_values", I64, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_array_shift_f64", DOUBLE, &[I64]);
    module.declare_function("js_set_alloc", I64, &[I32]);
    module.declare_function("js_set_from_array", I64, &[I64]);
    module.declare_function("js_set_from_iterable", I64, &[DOUBLE]);
    module.declare_function("js_map_from_array", I64, &[I64]);
    module.declare_function("js_map_from_iterable", I64, &[DOUBLE]);
    module.declare_function("js_object_has_property", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_in_operator", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_private_brand_check", DOUBLE, &[DOUBLE, I32, PTR, I32]);
    module.declare_function(
        "js_private_guard",
        DOUBLE,
        &[DOUBLE, I32, PTR, I32, I32, I32],
    );
    module.declare_function("js_fs_to_unix_timestamp", DOUBLE, &[DOUBLE]);
    module.declare_function("js_fs_write_file_sync", I32, &[DOUBLE, DOUBLE]);
    module.declare_function(
        "js_fs_write_file_sync_options",
        I32,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    // fs.appendFileSync(path, content) — returns i32 status. Issue #226.
    module.declare_function("js_fs_append_file_sync", I32, &[DOUBLE, DOUBLE]);
    module.declare_function(
        "js_fs_append_file_sync_options",
        I32,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_fs_exists_sync", I32, &[DOUBLE]);
    // fs.readFileSync(path, encoding) — returns a raw *mut StringHeader i64.
    module.declare_function("js_fs_read_file_sync", I64, &[DOUBLE]);
    module.declare_function("js_fs_read_file_dispatch", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_fs_open_as_blob", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_fs_promises_read_file", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function(
        "js_fs_promises_write_file",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_fs_promises_append_file",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_fs_promises_mkdir", DOUBLE, &[DOUBLE, DOUBLE]);
    // fs.mkdirSync(path) — returns i32 status (1=success).
    module.declare_function("js_fs_mkdir_sync", I32, &[DOUBLE]);
    module.declare_function("js_fs_mkdir_sync_options", I32, &[DOUBLE, DOUBLE]);
    // fs.unlinkSync(path) — returns i32 status.
    module.declare_function("js_fs_unlink_sync", I32, &[DOUBLE]);
    // fs.readdirSync(path, options) — returns NaN-boxed array of
    // strings, or array of Dirent objects when
    // `options.withFileTypes === true` (issue #631).
    module.declare_function("js_fs_readdir_sync", DOUBLE, &[DOUBLE, DOUBLE]);
    // fs.statSync(path) — returns a NaN-boxed object with isFile/isDirectory/size fields.
    module.declare_function("js_fs_stat_sync", DOUBLE, &[DOUBLE]);
    module.declare_function("js_fs_stat_sync_options", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_fs_lstat_sync", DOUBLE, &[DOUBLE]);
    module.declare_function("js_fs_lstat_sync_options", DOUBLE, &[DOUBLE, DOUBLE]);
    // fs.renameSync(from, to) — returns i32 status.
    module.declare_function("js_fs_rename_sync", I32, &[DOUBLE, DOUBLE]);
    // fs.copyFileSync(from, to) — returns i32 status.
    module.declare_function("js_fs_copy_file_sync", I32, &[DOUBLE, DOUBLE]);
    module.declare_function("js_fs_copy_file_sync_flags", I32, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_fs_cp_sync", I32, &[DOUBLE, DOUBLE]);
    module.declare_function("js_fs_cp_sync_options", I32, &[DOUBLE, DOUBLE, DOUBLE]);
    // fs.chmodSync(path, mode) — returns i32 status.
    module.declare_function("js_fs_chmod_sync", I32, &[DOUBLE, DOUBLE]);
    module.declare_function("js_fs_chown_sync", I32, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_fs_lchown_sync", I32, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_fs_lchmod_sync", I32, &[DOUBLE, DOUBLE]);
    module.declare_function("js_fs_truncate_sync", I32, &[DOUBLE, DOUBLE]);
    module.declare_function("js_fs_ftruncate_sync", I32, &[DOUBLE, DOUBLE]);
    module.declare_function("js_fs_fsync_sync", I32, &[DOUBLE]);
    module.declare_function("js_fs_fdatasync_sync", I32, &[DOUBLE]);
    module.declare_function("js_fs_fchmod_sync", I32, &[DOUBLE, DOUBLE]);
    module.declare_function("js_fs_fchown_sync", I32, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_fs_fstat_sync", DOUBLE, &[DOUBLE]);
    module.declare_function("js_fs_fstat_sync_options", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_fs_utimes_sync", I32, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_fs_lutimes_sync", I32, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_fs_futimes_sync", I32, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_fs_to_unix_timestamp", DOUBLE, &[DOUBLE]);
    module.declare_function("js_fs_readv_sync", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_fs_writev_sync", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_fs_statfs_sync", DOUBLE, &[DOUBLE]);
    module.declare_function("js_fs_statfs_sync_options", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_fs_opendir_sync", DOUBLE, &[DOUBLE]);
    module.declare_function("js_fs_glob_sync", DOUBLE, &[DOUBLE]);
    module.declare_function("js_fs_glob_sync_options", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_fs_link_sync", I32, &[DOUBLE, DOUBLE]);
    module.declare_function("js_fs_symlink_sync", I32, &[DOUBLE, DOUBLE]);
    module.declare_function(
        "js_fs_symlink_callback",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_fs_readlink_sync", I64, &[DOUBLE]);
    module.declare_function("js_fs_readlink_sync_options", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_fs_readlink_dispatch", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_fs_open_sync", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_fs_close_sync", I32, &[DOUBLE]);
    module.declare_function(
        "js_fs_read_sync",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_fs_read_sync_options", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_fs_write_sync", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function(
        "js_fs_write_string_sync_options",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_fs_write_buffer_sync",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_fs_write_sync_options_dispatch",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    // fs.accessSync(path) — returns i32 status (1=ok, 0=error).
    module.declare_function("js_fs_access_sync", I32, &[DOUBLE]);
    module.declare_function("js_fs_access_sync_mode", I32, &[DOUBLE, DOUBLE]);
    // fs.accessSync(path) — Node-compatible variant that throws on
    // failure (via js_throw → setjmp longjmp). Returns NaN-boxed undefined.
    module.declare_function("js_fs_access_sync_throw", DOUBLE, &[DOUBLE]);
    module.declare_function("js_fs_access_sync_throw_mode", DOUBLE, &[DOUBLE, DOUBLE]);
    // fs.realpathSync(path) — returns raw *mut StringHeader i64.
    module.declare_function("js_fs_realpath_sync", I64, &[DOUBLE]);
    module.declare_function("js_fs_realpath_dispatch", DOUBLE, &[DOUBLE, DOUBLE]);
    // fs.mkdtempSync(prefix) — returns raw *mut StringHeader i64.
    module.declare_function("js_fs_mkdtemp_sync", I64, &[DOUBLE]);
    module.declare_function("js_fs_mkdtemp_sync_options", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_fs_mkdtemp_dispatch", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_fs_mkdtemp_disposable_sync", DOUBLE, &[DOUBLE, DOUBLE]);
    // fs.rmdirSync(path) — returns i32 status.
    module.declare_function("js_fs_rmdir_sync", I32, &[DOUBLE]);
    module.declare_function("js_fs_rmdir_sync_options", I32, &[DOUBLE, DOUBLE]);
    // fs.rmRecursive(path) — recursive remove; returns i32 (1=ok, 0=fail).
    module.declare_function("js_fs_rm_recursive", I32, &[DOUBLE]);
    module.declare_function("js_fs_rm_recursive_options", I32, &[DOUBLE, DOUBLE]);
    // fs.createWriteStream(path) — returns NaN-boxed stream object.
    module.declare_function("js_fs_create_write_stream", DOUBLE, &[DOUBLE, DOUBLE]);
    // fs.createReadStream(path[, options]) — returns NaN-boxed stream object.
    module.declare_function("js_fs_create_read_stream", DOUBLE, &[DOUBLE, DOUBLE]);
    // fs.Utf8Stream constructor dispatch.
    module.declare_function("js_fs_utf8_stream_new", DOUBLE, &[DOUBLE]);
    module.declare_function("js_fs_utf8_stream_call_without_new", DOUBLE, &[DOUBLE]);
    module.declare_function("js_fs_utf8_stream_write", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_fs_utf8_stream_flush", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_fs_utf8_stream_flush_sync", DOUBLE, &[DOUBLE]);
    module.declare_function("js_fs_utf8_stream_end", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_fs_utf8_stream_destroy", DOUBLE, &[DOUBLE]);
    module.declare_function("js_fs_utf8_stream_reopen", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_fs_utf8_stream_on", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_fs_utf8_stream_once", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_fs_utf8_stream_off", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_fs_utf8_stream_remove_all", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function(
        "js_fs_utf8_stream_listener_count",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    module.declare_function("js_fs_utf8_stream_emit", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    // fs.readFile(path, encoding, callback) — Node-compatible callback variant.
    module.declare_function(
        "js_fs_read_file_callback",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    // Stats helper: method dispatcher called from the LLVM dispatch fast path.
    module.declare_function("js_fs_stats_is_file", DOUBLE, &[DOUBLE]);
    module.declare_function("js_fs_stats_is_directory", DOUBLE, &[DOUBLE]);
    // fs.readFileSync(path) with no encoding — returns a raw *mut BufferHeader
    // that the runtime's format_jsvalue path recognizes via BUFFER_REGISTRY
    // and prints as `<Buffer xx xx ...>`.
    module.declare_function("js_fs_read_file_binary", I64, &[DOUBLE]);
    module.declare_function("js_number_coerce", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_to_number", DOUBLE, &[DOUBLE]);
    module.declare_function("js_set_add", I64, &[I64, DOUBLE]);
    module.declare_function("js_set_add_string", I64, &[I64, I64]);
    module.declare_function("js_set_add_number", I64, &[I64, DOUBLE]);
    module.declare_function("js_set_add_i32", I64, &[I64, I32]);
    module.declare_function("js_set_add_u32", I64, &[I64, I32]);
    module.declare_function("js_set_add_f32", I64, &[I64, F32]);
    module.declare_function("js_set_add_bool", I64, &[I64, I32]);
    module.declare_function("js_set_has", I32, &[I64, DOUBLE]);
    module.declare_function("js_set_has_string", I32, &[I64, I64]);
    module.declare_function("js_set_has_number", I32, &[I64, DOUBLE]);
    module.declare_function("js_set_has_i32", I32, &[I64, I32]);
    module.declare_function("js_set_has_u32", I32, &[I64, I32]);
    module.declare_function("js_set_has_f32", I32, &[I64, F32]);
    module.declare_function("js_set_has_bool", I32, &[I64, I32]);
    module.declare_function("js_set_delete", I32, &[I64, DOUBLE]);
    module.declare_function("js_set_delete_string", I32, &[I64, I64]);
    module.declare_function("js_set_delete_number", I32, &[I64, DOUBLE]);
    module.declare_function("js_set_delete_i32", I32, &[I64, I32]);
    module.declare_function("js_set_delete_u32", I32, &[I64, I32]);
    module.declare_function("js_set_delete_f32", I32, &[I64, F32]);
    module.declare_function("js_set_delete_bool", I32, &[I64, I32]);
    module.declare_function("js_set_size", I32, &[I64]);
    // #2872: ES2024 Set composition methods.
    module.declare_function("js_set_union", I64, &[I64, DOUBLE]);
    module.declare_function("js_set_intersection", I64, &[I64, DOUBLE]);
    module.declare_function("js_set_difference", I64, &[I64, DOUBLE]);
    module.declare_function("js_set_symmetric_difference", I64, &[I64, DOUBLE]);
    module.declare_function("js_set_is_subset_of", I32, &[I64, DOUBLE]);
    module.declare_function("js_set_is_superset_of", I32, &[I64, DOUBLE]);
    module.declare_function("js_set_is_disjoint_from", I32, &[I64, DOUBLE]);
    module.declare_function("js_string_to_lower_case", I64, &[I64]);
    module.declare_function("js_string_to_upper_case", I64, &[I64]);
    // Locale-aware casing + locales validation (#2781). The locales arg is a
    // NaN-boxed JSValue (string / array / undefined).
    module.declare_function("js_string_to_locale_lower_case", I64, &[I64, DOUBLE]);
    module.declare_function("js_string_to_locale_upper_case", I64, &[I64, DOUBLE]);
    module.declare_function("js_string_validate_locales", VOID, &[DOUBLE]);
    module.declare_function("js_string_trim", I64, &[I64]);
    module.declare_function("js_string_trim_start", I64, &[I64]);
    module.declare_function("js_string_trim_end", I64, &[I64]);
    // Annex B §B.2.2 HTML wrappers. No-arg tag wrappers take only the receiver;
    // anchor/link/fontcolor/fontsize take an already-coerced attribute-value
    // string handle as the 2nd arg.
    module.declare_function("js_string_big", I64, &[I64]);
    module.declare_function("js_string_blink", I64, &[I64]);
    module.declare_function("js_string_bold", I64, &[I64]);
    module.declare_function("js_string_fixed", I64, &[I64]);
    module.declare_function("js_string_italics", I64, &[I64]);
    module.declare_function("js_string_small", I64, &[I64]);
    module.declare_function("js_string_strike", I64, &[I64]);
    module.declare_function("js_string_sub", I64, &[I64]);
    module.declare_function("js_string_sup", I64, &[I64]);
    module.declare_function("js_string_anchor", I64, &[I64, I64]);
    module.declare_function("js_string_link", I64, &[I64, I64]);
    module.declare_function("js_string_fontcolor", I64, &[I64, I64]);
    module.declare_function("js_string_fontsize", I64, &[I64, I64]);
    module.declare_function("js_string_char_at", I64, &[I64, I32]);
    // #3987: `s[key]` canonical-index read — returns the char (NaN-boxed string)
    // for a valid array index, else NaN-boxed `undefined`. Takes the raw key.
    module.declare_function("js_string_index_get", DOUBLE, &[I64, DOUBLE]);
    // #2787: NaN-safe JS index coercion (undefined/NaN -> 0, trunc, clamp) for
    // the char-access methods, replacing a raw `fptosi` that is UB on a NaN.
    module.declare_function("js_string_index_to_i32", I32, &[DOUBLE]);
    // `end`-arg coercion for slice/substring: `undefined` -> `len` (not 0), else
    // ToIntegerOrInfinity. `(value, len) -> i32`.
    module.declare_function("js_string_end_index_to_i32", I32, &[DOUBLE, I32]);
    // Issue #514: tag-aware dynamic index dispatch — routes `obj[idx]` to
    // `js_string_char_at` / `js_array_get_f64` / `js_object_get_field_by_name_f64`
    // based on the receiver's NaN-box tag at runtime. Used by IndexGet's
    // fallback path when codegen can't statically prove the receiver type.
    module.declare_function("js_dyn_index_get", DOUBLE, &[DOUBLE, DOUBLE]);
    // Issue #957: tag-aware dynamic index write. Used by `Expr::IndexUpdate`
    // codegen to write back the incremented value without rebuilding the
    // IndexSet dispatch tree. Routes to `js_array_set_index_or_string` for
    // arrays and `js_object_set_field_by_name` for plain objects.
    module.declare_function("js_dyn_index_set", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_string_to_char_array", I64, &[I64]);
    module.declare_function("js_string_repeat", I64, &[I64, DOUBLE]);
    module.declare_function("js_string_replace_string", I64, &[I64, I64, I64]);
    module.declare_function("js_string_replace_all_string", I64, &[I64, I64, I64]);
    module.declare_function("js_string_equals", I32, &[I64, I64]);
    module.declare_function("js_string_compare", I32, &[I64, I64]);
    module.declare_function("js_jsvalue_to_string_radix", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_math_random", DOUBLE, &[]);
    // WebAssembly host runtime (issue #76). All take/return NaN-boxed
    // doubles (JSValues). Implementations live in
    // `perry-runtime/src/webassembly.rs` and forward to
    // `perry-wasm-host`'s C ABI; the wasmi engine is only linked when
    // the user passes `--enable-wasm-runtime`.
    module.declare_function("js_webassembly_validate", DOUBLE, &[DOUBLE]);
    module.declare_function("js_webassembly_compile", DOUBLE, &[DOUBLE]);
    module.declare_function("js_webassembly_module_new", DOUBLE, &[DOUBLE]);
    module.declare_function("js_webassembly_module_exports", DOUBLE, &[DOUBLE]);
    module.declare_function("js_webassembly_module_imports", DOUBLE, &[DOUBLE]);
    module.declare_function(
        "js_webassembly_module_custom_sections",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    module.declare_function("js_webassembly_instantiate", DOUBLE, &[DOUBLE]);
    module.declare_function("js_webassembly_call_export_0", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function(
        "js_webassembly_call_export_1",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_webassembly_call_export_2",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_webassembly_call_export_3",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_webassembly_call_export_4",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_console_log_spread", VOID, &[I64]);
    module.declare_function("js_console_info_spread", VOID, &[I64]);
    module.declare_function("js_console_debug_spread", VOID, &[I64]);
    module.declare_function("js_console_error_spread", VOID, &[I64]);
    module.declare_function("js_console_warn_spread", VOID, &[I64]);
    // #1002: util format helpers receive console-style spread args.
    module.declare_function("js_util_format", DOUBLE, &[I64]);
    module.declare_function("js_util_format_with_options", DOUBLE, &[DOUBLE, I64]);
    module.declare_function("js_util_inspect", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_util_debuglog", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_util_diff", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_util_inherits", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_util_is_deep_strict_equal", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_util_strip_vt_control_characters", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_style_text", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_util_get_call_sites", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_util_promisify", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_callbackify", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_deprecate", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_util_aborted", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_util_transferable_abort_controller", DOUBLE, &[]);
    module.declare_function("js_util_transferable_abort_signal", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_parse_args", DOUBLE, &[DOUBLE]);
    module.declare_function("js_boxed_number_new", DOUBLE, &[DOUBLE]);
    module.declare_function("js_boxed_string_new", DOUBLE, &[DOUBLE, I32]);
    module.declare_function("js_boxed_boolean_new", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_arguments_object", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_promise", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_big_int_object", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_symbol_object", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_array_buffer", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_shared_array_buffer", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_any_array_buffer", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_array_buffer_view", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_data_view", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_typed_array", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_uint8_array", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_int8_array", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_int16_array", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_uint16_array", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_int32_array", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_uint32_array", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_float16_array", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_float32_array", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_float64_array", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_uint8_clamped_array", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_big_int64_array", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_big_uint64_array", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_map", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_weak_map", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_set", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_weak_set", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_date", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_reg_exp", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_async_function", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_generator_function", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_generator_object", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_native_error", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_number_object", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_string_object", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_boolean_object", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_boxed_primitive", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_external", DOUBLE, &[DOUBLE]);
    module.declare_function(
        "js_util_types_is_module_namespace_object",
        DOUBLE,
        &[DOUBLE],
    );
    module.declare_function("js_util_types_is_key_object", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_crypto_key", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_proxy", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_map_iterator", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_set_iterator", DOUBLE, &[DOUBLE]);
    module.declare_function("js_data_view_new", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_getenv", I64, &[I64]);
    module.declare_function("js_getenv_value", DOUBLE, &[I64]);
    // #1344: process.env.X = v / delete process.env.X.
    module.declare_function("js_setenv", VOID, &[I64, DOUBLE]);
    module.declare_function("js_removeenv", VOID, &[I64]);
    // #1350: process.exitCode get/set.
    module.declare_function("js_process_exit_code_get", DOUBLE, &[]);
    module.declare_function("js_process_exit_code_set", DOUBLE, &[DOUBLE]);
    module.declare_function("js_console_table", VOID, &[DOUBLE]);
    module.declare_function("js_console_table_with_properties", VOID, &[DOUBLE, DOUBLE]);
    module.declare_function("js_console_trace", VOID, &[DOUBLE]);
    module.declare_function("js_console_trace_spread", VOID, &[I64]);
    // process.* — see `perry-runtime/src/os.rs` and `perry-runtime/src/process.rs`.
    // Most process accessors return raw pointers (I64) that the call site
    // must NaN-box. The ones that return already-boxed f64 values
    // (`js_process_versions`, `js_process_memory_usage`, `js_process_hrtime_bigint`,
    // `js_process_stdin/out/err`) are declared as DOUBLE.
    module.declare_function("js_process_cwd", I64, &[]);
    module.declare_function("js_process_argv", I64, &[]);
    module.declare_function("js_process_pid", DOUBLE, &[]);
    module.declare_function("js_process_ppid", DOUBLE, &[]);
    module.declare_function("js_process_uptime", DOUBLE, &[]);
    module.declare_function("js_process_version", I64, &[]);
    module.declare_function("js_process_versions", DOUBLE, &[]);
    module.declare_function("js_process_memory_usage", DOUBLE, &[]);
    // node:v8 (#3137/#3138/#3142).
    module.declare_function("js_bigint_as_int_n_call", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_bigint_as_uint_n_call", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_v8_serialize", DOUBLE, &[DOUBLE]);
    module.declare_function("js_v8_deserialize", DOUBLE, &[DOUBLE]);
    module.declare_function("js_v8_get_heap_statistics", DOUBLE, &[]);
    module.declare_function("js_v8_get_heap_code_statistics", DOUBLE, &[]);
    module.declare_function("js_v8_get_heap_space_statistics", DOUBLE, &[]);
    module.declare_function("js_v8_cached_data_version_tag", DOUBLE, &[]);
    module.declare_function("js_v8_get_heap_snapshot", DOUBLE, &[DOUBLE]);
    module.declare_function("js_v8_write_heap_snapshot", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_v8_gc_profiler_new", DOUBLE, &[]);
    module.declare_function("js_v8_gc_profiler_start", DOUBLE, &[DOUBLE]);
    module.declare_function("js_v8_gc_profiler_stop", DOUBLE, &[DOUBLE]);
    module.declare_function("js_v8_gc_profiler_report", DOUBLE, &[]);
    // node:v8 Serializer/Deserializer classes (#3680) + lifecycle/diagnostic (#3679).
    module.declare_function("js_v8_serializer_new", DOUBLE, &[DOUBLE]);
    module.declare_function("js_v8_deserializer_new", DOUBLE, &[DOUBLE]);
    module.declare_function("js_v8_noop_undefined", DOUBLE, &[]);
    module.declare_function("js_v8_is_building_snapshot", DOUBLE, &[]);
    module.declare_function("js_v8_namespace", DOUBLE, &[PTR, I64]);
    module.declare_function("js_v8_throw_not_building_snapshot", DOUBLE, &[]);
    module.declare_function("js_v8_promise_hook_register", DOUBLE, &[]);
    module.declare_function("js_v8_promise_hooks_on_init", DOUBLE, &[DOUBLE]);
    module.declare_function("js_v8_promise_hooks_on_before", DOUBLE, &[DOUBLE]);
    module.declare_function("js_v8_promise_hooks_on_after", DOUBLE, &[DOUBLE]);
    module.declare_function("js_v8_promise_hooks_on_settled", DOUBLE, &[DOUBLE]);
    module.declare_function("js_v8_promise_hooks_create_hook", DOUBLE, &[DOUBLE]);
    module.declare_function("js_process_thread_cpu_usage", DOUBLE, &[DOUBLE]);
    module.declare_function("js_process_available_memory", DOUBLE, &[]);
    module.declare_function("js_process_constrained_memory", DOUBLE, &[]);
    module.declare_function("js_process_source_maps_enabled", DOUBLE, &[]);
    module.declare_function("js_process_set_source_maps_enabled", DOUBLE, &[DOUBLE]);
    module.declare_function(
        "js_process_has_uncaught_exception_capture_callback",
        DOUBLE,
        &[],
    );
    module.declare_function(
        "js_process_set_uncaught_exception_capture_callback",
        DOUBLE,
        &[DOUBLE],
    );
    module.declare_function(
        "js_process_add_uncaught_exception_capture_callback",
        DOUBLE,
        &[DOUBLE],
    );
    module.declare_function("js_process_getuid", DOUBLE, &[]);
    module.declare_function("js_process_geteuid", DOUBLE, &[]);
    module.declare_function("js_process_getgid", DOUBLE, &[]);
    module.declare_function("js_process_getegid", DOUBLE, &[]);
    module.declare_function("js_process_emit_warning", VOID, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_process_cpu_usage", DOUBLE, &[DOUBLE]);
    module.declare_function("js_process_resource_usage", DOUBLE, &[]);
    module.declare_function("js_process_active_resources_info", DOUBLE, &[]);
    module.declare_function("js_process_env", DOUBLE, &[]);
    module.declare_function("js_process_hrtime_bigint", DOUBLE, &[]);
    module.declare_function("js_process_hrtime", DOUBLE, &[DOUBLE]);
    module.declare_function("js_process_title", DOUBLE, &[]);
    module.declare_function("js_process_set_title", VOID, &[DOUBLE]);
    module.declare_function("js_process_chdir", VOID, &[I64]);
    // #2013 — f64-taking variant that validates type before dispatch.
    module.declare_function("js_process_chdir_jsv", VOID, &[DOUBLE]);
    module.declare_function("js_process_kill", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_process_exit", VOID, &[DOUBLE]);
    module.declare_function("js_process_abort", VOID, &[]);
    module.declare_function("js_process_umask", DOUBLE, &[]);
    module.declare_function("js_process_umask_set", DOUBLE, &[DOUBLE]);
    module.declare_function("js_process_on", DOUBLE, &[I64, I64]);
    module.declare_function("js_process_add_listener", DOUBLE, &[I64, I64]);
    module.declare_function("js_process_once", DOUBLE, &[I64, I64]);
    module.declare_function("js_process_prepend_listener", DOUBLE, &[I64, I64]);
    module.declare_function("js_process_prepend_once_listener", DOUBLE, &[I64, I64]);
    module.declare_function("js_process_emit", DOUBLE, &[I64, I64]);
    module.declare_function("js_process_emit_before_exit", VOID, &[DOUBLE]);
    module.declare_function("js_process_run_finalization_exit", VOID, &[]);
    module.declare_function("js_promise_report_unhandled_rejections", VOID, &[]);
    module.declare_function("js_process_remove_listener", DOUBLE, &[I64, I64]);
    module.declare_function("js_process_off", DOUBLE, &[I64, I64]);
    module.declare_function("js_process_remove_all_listeners", DOUBLE, &[I64]);
    module.declare_function("js_process_listener_count", DOUBLE, &[I64, I64]);
    module.declare_function("js_process_listeners", I64, &[I64]);
    module.declare_function("js_process_raw_listeners", I64, &[I64]);
    module.declare_function("js_process_event_names", I64, &[]);
    module.declare_function("js_process_set_max_listeners", DOUBLE, &[DOUBLE]);
    module.declare_function("js_process_get_max_listeners", DOUBLE, &[]);
    module.declare_function("js_process_get_builtin_module", DOUBLE, &[DOUBLE]);
    module.declare_function("js_process_get_builtin_module_devirt", DOUBLE, &[DOUBLE]);
    module.declare_function("js_process_execve", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    // #3108: process.sourceMapsEnabled getter + setSourceMapsEnabled(bool).
    module.declare_function("js_process_source_maps_enabled", DOUBLE, &[]);
    module.declare_function("js_process_set_source_maps_enabled", DOUBLE, &[DOUBLE]);
    module.declare_function("js_module_is_builtin", DOUBLE, &[DOUBLE]);
    module.declare_function("js_module_find_package_json", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_module_register", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_module_register_hooks", DOUBLE, &[DOUBLE]);
    module.declare_function("js_process_next_tick", VOID, &[I64, I64]);
    module.declare_function("js_process_stdin", DOUBLE, &[]);
    module.declare_function("js_process_stdout", DOUBLE, &[]);
    module.declare_function("js_process_stderr", DOUBLE, &[]);
    // readline (#347) — Phase 2 raw-mode toggle + stdin event handlers.
    module.declare_function("js_readline_set_raw_mode", DOUBLE, &[DOUBLE]);
    module.declare_function("js_readline_stdin_on", VOID, &[I64, I64]);
    module.declare_function("js_readline_stdin_remove_listener", DOUBLE, &[I64, I64]);
    module.declare_function("js_readline_stdin_pause", DOUBLE, &[]);
    module.declare_function("js_readline_stdin_resume", DOUBLE, &[]);
    module.declare_function("js_readline_stdin_unref", DOUBLE, &[]);
    module.declare_function("js_readline_stdin_ref", DOUBLE, &[]);
    module.declare_function("js_readline_stdin_destroy", DOUBLE, &[]);
    // tty (#347 Phase 3) — isatty + stdout dimensions + resize handler.
    module.declare_function("js_tty_isatty", DOUBLE, &[DOUBLE]);
    module.declare_function("js_tty_read_stream_new", DOUBLE, &[DOUBLE]);
    module.declare_function("js_tty_write_stream_new", DOUBLE, &[DOUBLE]);
    module.declare_function("js_wasi_constructor_call", DOUBLE, &[DOUBLE]);
    module.declare_function("js_process_stdin_isatty", DOUBLE, &[]);
    module.declare_function("js_process_stdout_isatty", DOUBLE, &[]);
    module.declare_function("js_process_stderr_isatty", DOUBLE, &[]);
    module.declare_function("js_process_stdout_columns", DOUBLE, &[]);
    module.declare_function("js_process_stdout_rows", DOUBLE, &[]);
    module.declare_function("js_process_stdout_on", DOUBLE, &[I64, I64]);
    // os.* — also used by Expr::OsArch/Type/Platform/Release/Hostname/EOL.
    module.declare_function("js_os_platform", I64, &[]);
    module.declare_function("js_os_arch", I64, &[]);
    module.declare_function("js_os_type", I64, &[]);
    module.declare_function("js_os_release", I64, &[]);
    module.declare_function("js_os_hostname", I64, &[]);
    module.declare_function("js_os_eol", I64, &[]);
    module.declare_function("js_os_available_parallelism", DOUBLE, &[]);
    module.declare_function("js_os_endianness", I64, &[]);
    module.declare_function("js_os_dev_null", I64, &[]);
    module.declare_function("js_os_machine", I64, &[]);
    module.declare_function("js_os_loadavg", I64, &[]);
    module.declare_function("js_os_version", I64, &[]);
    // Heap-allocated mutable capture boxes.
    // See crates/perry-runtime/src/box.rs. These let multiple
    // closures share mutable state (e.g. a counter captured by
    // both inc() and get() in a returned object literal).
    module.declare_function("js_box_alloc_bits", I64, &[I64]);
    module.declare_function("js_box_get_bits", I64, &[I64]);
    module.declare_function("js_box_set_bits", VOID, &[I64, I64]);
    module.declare_function("js_box_alloc", I64, &[DOUBLE]);
    module.declare_function("js_box_get", DOUBLE, &[I64]);
    module.declare_function("js_box_set", VOID, &[I64, DOUBLE]);
    module.declare_function("js_i32_box_alloc", I64, &[I32]);
    module.declare_function("js_i32_box_get", I32, &[I64]);
    module.declare_function("js_i32_box_set", VOID, &[I64, I32]);
    module.declare_function("js_bool_box_alloc", I64, &[I32]);
    module.declare_function("js_bool_box_get", I32, &[I64]);
    module.declare_function("js_bool_box_set", VOID, &[I64, I32]);
    module.declare_function("js_arguments_object_alloc", I64, &[DOUBLE, DOUBLE, I32]);
    module.declare_function("js_arguments_object_map_index", VOID, &[I64, I32, I64]);
    module.declare_function("js_array_like_to_array", I64, &[DOUBLE]);
    module.declare_function("js_object_get_class_id", I32, &[I64]);
    module.declare_function("js_object_alloc_with_parent", I64, &[I32, I32, I32]);
    // Class instance allocator that pre-populates the keys_array with
    // the class's field names. Required so the LLVM PropertyGet/Set
    // fast path's slot indices match the runtime's by-name dispatch
    // (which walks keys_array). Without this, classes that mix
    // fast-path field access with runtime-helper field access (e.g.
    // PropertySet via fast path + PropertyUpdate via runtime) end up
    // reading/writing different slots for the same field name.
    module.declare_function(
        "js_object_alloc_class_with_keys",
        I64,
        &[I32, I32, I32, PTR, I32],
    );
    // Wall 45: merged-layout allocator for dynamic-parent subclasses
    // (`class X extends _mod.default`). Args: (class_id, own_field_count,
    // own_packed_keys, own_packed_keys_len). Resolves the runtime-registered
    // parent layout and allocates `[parent keys..] ++ [own keys..]` so
    // inherited fields land at the parent's slot indices.
    module.declare_function(
        "js_object_alloc_class_dynamic_parent",
        I64,
        &[I32, I32, PTR, I32],
    );
    // Fast class allocator that takes a pre-built keys_array pointer
    // directly, bypassing the per-call SHAPE_CACHE lookup. The codegen
    // emits one `js_build_class_keys_array` call at module init per
    // class, stores the result in a per-class global, then uses this
    // function on every `new ClassName()` call.
    module.declare_function(
        "js_object_alloc_class_inline_keys",
        I64,
        &[I32, I32, I32, I64],
    );
    module.declare_function("js_build_class_keys_array", I64, &[I32, I32, PTR, I32]);
    // Inline bump-allocator state accessor + slow path. The codegen
    // calls `js_inline_arena_state` once per JS function entry, caches
    // the returned pointer in a stack slot, and reads/writes the
    // bump-pointer state directly via fixed GEPs (data=0, offset=8,
    // size=16). When the bump check fails, it calls
    // `js_inline_arena_slow_alloc` which syncs back to the underlying
    // arena, allocates a new block, and returns the new pointer.
    //
    // The runtime structs live in `crates/perry-runtime/src/arena.rs`.
    // Field offsets are load-bearing — keep `#[repr(C)] InlineArenaState`
    // in sync with the GEPs we emit in `lower_call::compile_new`.
    module.declare_function("js_inline_arena_state", PTR, &[]);
    module.declare_function("js_inline_arena_slow_alloc", PTR, &[PTR, I64, I64]);
    module.declare_function("js_object_delete_field", I32, &[I64, I64]);
    // Primitive-safe `delete` wrappers: take the RAW NaN-boxed receiver (DOUBLE)
    // so `delete (number).x` / `delete (number)[k]` no-op to `true` instead of
    // unboxing the primitive's bits as a garbage ObjectHeader* (EXC_BAD_ACCESS).
    module.declare_function("js_object_delete_field_value", I32, &[DOUBLE, I64]);
    module.declare_function("js_object_delete_dynamic_value", I32, &[DOUBLE, DOUBLE]);
    // Box a `delete` success bit into a JS boolean, throwing TypeError in
    // strict mode when the delete was refused (non-configurable property).
    module.declare_function("js_delete_result", DOUBLE, &[I32, I32]);
    // js_eq takes JSValue (#[repr(transparent)] u64) for both
    // params + return — i64 in the ABI, not double.
    module.declare_function("js_eq", I64, &[I64, I64]);
    module.declare_function("js_loose_eq", I64, &[I64, I64]);
    module.declare_function("js_number_to_fixed", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_string_replace_regex", I64, &[I64, I64, I64]);
    module.declare_function("js_string_replace_all_regex", I64, &[I64, I64, I64]);
    module.declare_function("js_array_at", DOUBLE, &[I64, DOUBLE]);
    // Date getters: all take a timestamp double, return a double.
    module.declare_function("js_date_get_time", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_full_year", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_month", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_date", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_day", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_hours", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_minutes", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_seconds", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_milliseconds", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_utc_day", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_utc_full_year", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_utc_month", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_utc_date", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_utc_hours", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_utc_minutes", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_utc_seconds", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_utc_milliseconds", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_value_of", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_timezone_offset", DOUBLE, &[DOUBLE]);
    // #2089: deref a Date (NaN-boxed DateCell pointer) to its ms timestamp for
    // ordered relational compares; a plain number passes through unchanged.
    module.declare_function("js_date_coerce_number", DOUBLE, &[DOUBLE]);
    // Abstract relational comparison (`<`, `<=`, `>`, `>=`) for operands that
    // aren't both statically numeric: full ToPrimitive / string / BigInt /
    // mixed-numeric semantics. Each returns a NaN-boxed boolean.
    module.declare_function("js_rel_lt", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_rel_le", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_rel_gt", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_rel_ge", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_date_to_string", I64, &[DOUBLE]);
    module.declare_function("js_date_to_iso_string", I64, &[DOUBLE]);
    module.declare_function("js_date_to_iso_string_or_throw", I64, &[DOUBLE]);
    module.declare_function("js_date_new_from_timestamp", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_new_from_value", DOUBLE, &[DOUBLE]);
    module.declare_function("js_array_indexOf_f64", I32, &[I64, DOUBLE]);
    // #2804: indexOf/includes carry an optional fromIndex (value, fromIndex, has_from).
    // Return I64 so indices >= 2^31 are not truncated.
    module.declare_function("js_array_indexOf_jsvalue", I64, &[I64, DOUBLE, DOUBLE, I32]);
    module.declare_function(
        "js_array_last_index_of_jsvalue",
        I64,
        &[I64, DOUBLE, DOUBLE, I32],
    );
    module.declare_function("js_array_includes_f64", I32, &[I64, DOUBLE]);
    module.declare_function(
        "js_array_includes_jsvalue",
        I32,
        &[I64, DOUBLE, DOUBLE, I32],
    );
    module.declare_function("js_map_size", I32, &[I64]);
    module.declare_function("js_map_clear", VOID, &[I64]);
    module.declare_function("js_set_clear", VOID, &[I64]);
    // Map iteration: entries/keys/values all take a map pointer and return an array pointer.
    module.declare_function("js_map_entries", I64, &[I64]);
    module.declare_function("js_map_keys", I64, &[I64]);
    module.declare_function("js_map_values", I64, &[I64]);
    // Direct entry access for the `for (const [k, v] of map)` fast path
    // — skips the pair-Array materialization that `js_map_entries`
    // would do. (Map ptr, entry idx) → key / value.
    module.declare_function("js_map_entry_key_at", DOUBLE, &[I64, I32]);
    module.declare_function("js_map_entry_value_at", DOUBLE, &[I64, I32]);
    // #6075: current index of a key/value (or -1) for the delete-safe for-of
    // fast path. Takes the NaN-boxed collection + key; strips internally.
    module.declare_function("js_map_find_key_index", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_set_find_value_index", DOUBLE, &[DOUBLE, DOUBLE]);
    // Map/Set forEach: (collection_ptr, callback_nanboxed_f64, thisArg_f64) -> void (#2830)
    module.declare_function("js_map_foreach", VOID, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_set_foreach", VOID, &[I64, DOUBLE, DOUBLE]);
    // #2856: value-level Map/Set iterator methods return a real iterator
    // OBJECT (raw ptr as i64; caller NaN-boxes), unlike the eager Array
    // materializers above which still back the for-of/spread fast paths.
    module.declare_function("js_map_entries_iter_obj", I64, &[I64]);
    module.declare_function("js_map_keys_iter_obj", I64, &[I64]);
    module.declare_function("js_map_values_iter_obj", I64, &[I64]);
    module.declare_function("js_set_values_iter_obj", I64, &[I64]);
    module.declare_function("js_set_keys_iter_obj", I64, &[I64]);
    module.declare_function("js_set_entries_iter_obj", I64, &[I64]);
    // Set to array conversion (for Set iteration via for...of)
    module.declare_function("js_set_to_array", I64, &[I64]);
    // Direct element access for the `for (const x of set)` fast path —
    // skips the throwaway Array allocation that `js_set_to_array` would do.
    module.declare_function("js_set_value_at", DOUBLE, &[I64, I32]);
    // Splice is unusual: takes an out-pointer for the deleted array
    // and returns the modified-in-place input (the splice point may
    // realloc). Param order is (arr, start, delete_count, items_ptr,
    // items_count, out_arr_ptr).
    module.declare_function("js_array_splice", I64, &[I64, I32, I32, PTR, I32, PTR]);
    module.declare_function("js_array_splice_delete_count", I32, &[DOUBLE]);
    module.declare_function("js_parse_int", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_parse_float", DOUBLE, &[I64]);
    module.declare_function("js_array_reduce", DOUBLE, &[I64, I64, I32, DOUBLE]);
    module.declare_function("js_array_reduce_right", DOUBLE, &[I64, I64, I32, DOUBLE]);
    module.declare_function("js_array_sort_default", I64, &[I64]);
    module.declare_function("js_array_reverse", I64, &[I64]);
    module.declare_function("js_array_reverse_value", DOUBLE, &[DOUBLE]);
    module.declare_function("js_array_flat", I64, &[I64]);
    module.declare_function("js_array_flat_depth", I64, &[I64, DOUBLE]);
    module.declare_function("js_array_flatMap", I64, &[I64, I64]);
    module.declare_function("js_array_sort_with_comparator", I64, &[I64, I64]);
    // #2796: validate sort/toSorted comparator (function | undefined) before sorting.
    module.declare_function("js_validate_array_comparator", I64, &[DOUBLE]);
    // #4091: non-callable callback validators for higher-order array /
    // TypedArray methods. Standard form takes the boxed callback; the `map`
    // form also takes the receiver handle so it can pick the typed-array
    // message format.
    module.declare_function("js_validate_array_callback", I64, &[DOUBLE]);
    module.declare_function("js_validate_array_map_callback", I64, &[I64, DOUBLE]);
    // ES2023 immutable array methods
    module.declare_function("js_array_to_reversed", I64, &[I64]);
    module.declare_function("js_array_to_sorted_default", I64, &[I64]);
    module.declare_function("js_array_to_sorted_with_comparator", I64, &[I64, I64]);
    module.declare_function("js_array_to_spliced", I64, &[I64, DOUBLE, DOUBLE, PTR, I32]);
    module.declare_function("js_array_with", I64, &[I64, DOUBLE, DOUBLE]);
    module.declare_function(
        "js_array_copy_within",
        I64,
        &[I64, DOUBLE, DOUBLE, I32, DOUBLE],
    );
    module.declare_function(
        "js_array_copy_within_value",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, I32, DOUBLE],
    );
    module.declare_function("js_regexp_new", I64, &[I64, I64]);
    // Full ECMAScript RegExp constructor: NaN-boxed pattern + flags in, handles
    // RegExp/undefined/object patterns and ToString-coerced flags.
    module.declare_function("js_regexp_construct", I64, &[DOUBLE, DOUBLE]);
    // Function-call form `RegExp(x)`: identity shortcut (a RegExp arg + undefined
    // flags returns the same object) then falls back to `js_regexp_construct`.
    module.declare_function("js_regexp_construct_call", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_regexp_test", I32, &[I64, I64]);
    // RegExp.escape(str) — #2899. Takes/returns NaN-boxed f64 (string).
    module.declare_function("js_regexp_escape", DOUBLE, &[DOUBLE]);
    module.declare_function("js_get_string_pointer_unified", I64, &[DOUBLE]);
    // Strict-equality (`===`) compare for switch case dispatch.
    module.declare_function("js_switch_strict_equals", I32, &[DOUBLE, DOUBLE]);
    module.declare_function("js_value_to_str_ptr_for_ffi", I64, &[DOUBLE]);
    // Closes #580: alias-on-copy refcount bump for string locals. The
    // call site at `crates/perry-codegen/src/stmt.rs:725` was added by
    // v0.5.667 (#536) to mark the source string as shared (refcount=0)
    // so `js_string_append`'s in-place fast path won't mutate a
    // still-aliased buffer when codegen detects `let y = x;` with a
    // string-typed source. Without this matching declare, every
    // `try { const y = x; ... } finally {...}` shape with a string
    // local failed clang with "use of undefined value
    // @js_string_addref" — pure linker-visible declaration fix.
    module.declare_function("js_string_addref", VOID, &[I64]);
    // Tag-checked addref taking a NaN-boxed value (DOUBLE): the scalar-replaced
    // object-field / array-element stores demote a possibly-unique string source
    // without materializing SSO strings. Same declaration rationale as above.
    module.declare_function("js_string_addref_if_heap_string", VOID, &[DOUBLE]);
    module.declare_function("js_bigint_from_string", I64, &[PTR, I32]);
    module.declare_function("js_bigint_from_f64", I64, &[DOUBLE]);
    module.declare_function("js_bigint_from_i128_parts", I64, &[I64, I64]);
    module.declare_function("js_bigint_cmp", I32, &[I64, I64]);
    // Dynamic bigint arithmetic — lowered from `Expr::Binary` when
    // either operand is statically bigint-typed. These unbox, call
    // the raw `js_bigint_<op>`, and re-box with BIGINT_TAG. Also
    // tolerate mixed bigint/int32 operands.
    module.declare_function("js_dynamic_add", DOUBLE, &[DOUBLE, DOUBLE]);
    // `++`/`--` slow path: ToNumeric read (BigInt passthrough, else ToNumber)
    // and a type-preserving step by 1n/1.0. Keeps `let i = 10n; i++` a BigInt.
    module.declare_function("js_to_numeric", DOUBLE, &[DOUBLE]);
    module.declare_function("js_numeric_step", DOUBLE, &[DOUBLE, I32]);
    // Refs #486: dispatch path for `+` when neither operand has a static
    // type (string|number|bigint). Per JS spec, string concat takes
    // priority; otherwise BigInt or numeric add. Hono's
    // `Node.buildRegExpStr` does `k + c.buildRegExpStr()` inside a for-of
    // loop where both operands lower as plain f64s with inferred type
    // Any — the static-string-concat fast path doesn't fire and the
    // numeric-fallback path coerced strings to NaN.
    module.declare_function("js_dynamic_string_or_number_add", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_dynamic_sub", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_dynamic_mul", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_dynamic_div", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_dynamic_mod", DOUBLE, &[DOUBLE, DOUBLE]);
    // Dynamic bigint bitwise ops — lowered from `Expr::Binary` when
    // either operand is statically bigint-typed. Unbox, call the raw
    // `js_bigint_<op>`, re-box with BIGINT_TAG. Fall through to i32
    // ToInt32 semantics for the pure-number case (closes #39).
    module.declare_function("js_dynamic_bitand", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_dynamic_bitor", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_dynamic_bitxor", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_dynamic_shl", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_dynamic_shr", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_dynamic_bitnot", DOUBLE, &[DOUBLE]);
    // #2908: `bigint ** bigint` (RangeError on negative exponent) and `>>>`
    // (always TypeError for BigInt operands). Numeric fallback inside.
    module.declare_function("js_dynamic_pow", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_dynamic_ushr", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_instanceof", DOUBLE, &[DOUBLE, I32]);
    // v0.5.749: dynamic instanceof — `value instanceof type` where type
    // is a runtime expression (function arg holding class ref).
    module.declare_function("js_instanceof_dynamic", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_instanceof_noncallable_rhs", DOUBLE, &[]);
    module.declare_function("js_register_class_extends_error", VOID, &[I32]);
    module.declare_function("js_register_class_extends_data_view", VOID, &[I32]);
    module.declare_function("js_register_class_id", VOID, &[I32]);
    // #1021 NestJS: surface Perry class names to V8 so `metatype.name`
    // is non-empty. Codegen emits one call per registered class id at
    // program init, mirroring `js_register_class_id`.
    module.declare_function("js_register_class_name", VOID, &[I32, PTR, I32]);
    // Anon-shape class registration so `.constructor` reads on object
    // literals (`{ x: 1 }`) return the global `Object` constructor
    // instead of the synthetic class ref. Refs #968 / date-fns
    // `constructFrom` blocker.
    module.declare_function("js_register_anon_shape_class_id", VOID, &[I32]);
    // Built-in constructor / namespace value-lookup on the globalThis
    // singleton. Used to wire `instance.constructor` and bare
    // `Date`/`Array`/`Object` identifiers to the same closure pointer
    // so `inst.constructor === Date` (date-fns / drizzle / lodash duck
    // checks) holds.
    module.declare_function("js_get_global_this_builtin_value", DOUBLE, &[PTR, I64]);
    module.declare_function("js_promise_static_function_value", DOUBLE, &[PTR, I64]);
    module.declare_function(
        "js_builtin_prototype_method_value",
        DOUBLE,
        &[PTR, I64, PTR, I64],
    );
    // Inline-allocator class registration: emitted once per class
    // with a parent in the entry-block init prelude. The runtime
    // allocators register on every alloc; the inline allocator skips
    // the alloc-site call and relies on this one-time registration.
    module.declare_function("js_register_class_parent", VOID, &[I32, I32]);
    // Issue #711: dynamic parent registration for `class X extends fn(...)`
    // shapes. Codegen emits at the class-declaration source position in
    // module.init (lower.rs); the runtime helper extracts the parent
    // class_id from the value (ClassRef payload or ObjectHeader.class_id)
    // and wires the (child, parent) edge into CLASS_REGISTRY.
    module.declare_function("js_register_class_parent_dynamic", VOID, &[I32, DOUBLE]);
    // Read back the parent VALUE stashed by `js_register_class_parent_dynamic`
    // at decl time, so `super()` resolves the parent from the module-init scope
    // instead of re-evaluating its extends expression in the constructor scope.
    module.declare_function("js_get_dynamic_parent_value", DOUBLE, &[I32]);
    // Decl-site snapshot of a function-nested class's captured locals —
    // consumed by the dynamic-construction replay (`new mod.C()`).
    module.declare_function("js_class_register_capture_values", VOID, &[I32, PTR, I64]);
    // #6052: TDZ-suppression window around the snapshot's capture loads — a
    // refresh emitted between two `let`/`const` initializers (the #6037
    // refresh-after-each-assignment strategy) legally reads a sibling capture
    // still in its dead zone; it must snapshot `undefined`, not throw.
    module.declare_function("js_tdz_suppress_begin", VOID, &[]);
    module.declare_function("js_tdz_suppress_end", VOID, &[]);
    // Static-method prologue read of one decl-site capture snapshot slot.
    module.declare_function("js_class_capture_value", DOUBLE, &[I32, I32]);
    // #5437: snapshot slot read with a `new`-site appended cap-arg fallback
    // (used when no decl-site snapshot was registered for the class).
    module.declare_function("js_class_capture_value_or", DOUBLE, &[I32, I32, DOUBLE]);
    // #5437 (param-first): the live `new`-site cap param wins when present; the
    // decl-site snapshot is consulted only when the param is `undefined` (the
    // cross-module construct path). Used by the synthesized ctor capture rebind.
    module.declare_function(
        "js_param_or_class_capture_value",
        DOUBLE,
        &[DOUBLE, I32, I32],
    );
    // `super(...spread)` — dynamic-arity ancestor ctor invocation on `this`.
    module.declare_function("js_super_construct_apply", VOID, &[I32, DOUBLE, DOUBLE]);
    // `super.method(...)` on a class with a runtime-registered (dynamic) parent
    // — resolve the method from the registered parent chain and call on `this`.
    module.declare_function(
        "js_super_method_call_dynamic",
        DOUBLE,
        &[I32, PTR, I64, DOUBLE, PTR, I64],
    );
    // `super.method(...spread)` — flatten the (codegen-built) args array into a
    // flat f64 buffer and forward to `js_super_method_call_dynamic`.
    module.declare_function(
        "js_super_method_call_dynamic_apply",
        DOUBLE,
        &[I32, PTR, I64, DOUBLE, DOUBLE],
    );
    module.declare_function("js_array_push_spread_any", I64, &[I64, DOUBLE]);
    // Issue #711 part 2: prototype-based class declaration via
    // `<func>.prototype = <obj>`. Binds an object as the function's
    // prototype source; subsequent `class X extends <func>` lookups
    // dispatch into the object's methods. Returns the synthetic
    // class id allocated for the function value (or 0 on validation
    // failure). Codegen discards the return.
    module.declare_function("js_set_function_prototype", I32, &[DOUBLE, DOUBLE]);
    // Issue #838: JS-classic prototype-method assignment.
    // `Class.prototype.method = fn` (or the aliased
    // `let p = Class.prototype; p.method = fn` shape) registers the
    // closure into a per-class side table consulted by the
    // `js_object_get_field_by_name` / `js_native_call_method` dispatch
    // hot paths so `(new Class()).method()` reaches the closure with
    // `this` bound to the receiver.
    module.declare_function(
        "js_register_prototype_method",
        VOID,
        &[I32, PTR, I64, DOUBLE],
    );
    // Issue #838 followup (b): function-classic prototype-method
    // assignment for Babel's class-from-function emit pattern (and
    // dayjs's identical minified shape). Takes the callable value
    // directly — the runtime helper looks up / allocates a synthetic
    // class id keyed by the closure's NaN-boxed bits, then stores
    // the method on `CLASS_PROTOTYPE_METHODS[synthetic_cid]`.
    // Returns the synthetic class id (codegen discards).
    module.declare_function(
        "js_register_function_prototype_method",
        I32,
        &[DOUBLE, PTR, I64, DOUBLE],
    );
    // Issue #838 followup (b): construct an instance from a function
    // value via the synthetic-class-id path. Allocates an object
    // stamped with the same synthetic id the
    // `js_register_function_prototype_method` site used, then
    // invokes the constructor with IMPLICIT_THIS bound to the new
    // instance. Returns the NaN-boxed new instance pointer.
    module.declare_function("js_new_function_construct", DOUBLE, &[DOUBLE, PTR, I64]);
    module.declare_function("js_function_ctor_from_strings", DOUBLE, &[PTR, I64]);
    // `new <callee>(...spread)` — codegen folds every argument (regular +
    // spread-expanded) into one JS array and hands it here; the runtime
    // materialises a flat buffer and forwards to `js_new_function_construct`.
    module.declare_function("js_new_function_construct_apply", DOUBLE, &[DOUBLE, DOUBLE]);
    // `new <primitive-literal>` → `TypeError: … is not a constructor`. Emitted
    // for primitive-literal callees (most importantly `f64` number literals,
    // which the runtime construct path cannot tag-distinguish from pointers).
    module.declare_function("js_throw_not_a_constructor", DOUBLE, &[]);
    module.declare_function("js_new_target_value", DOUBLE, &[]);
    // Read side of #838 followup (b): look up a previously-registered
    // prototype method on a function value by name. Pairs with
    // `js_register_function_prototype_method`. Returns the NaN-boxed
    // closure value if the synthetic-class-id derived from the function
    // has an entry under `name`, otherwise the NaN-boxed `undefined`
    // tag. ramda's transducer pattern + `typeof Foo.prototype.method`
    // introspection both reach this entry point.
    module.declare_function(
        "js_get_function_prototype_method",
        DOUBLE,
        &[DOUBLE, PTR, I64],
    );
    module.declare_function("js_function_prototype_value_for_read", DOUBLE, &[DOUBLE]);
    module.declare_function("js_typeerror_new", I64, &[I64]);
    module.declare_function("js_rangeerror_new", I64, &[I64]);
    module.declare_function("js_syntaxerror_new", I64, &[I64]);
    module.declare_function("js_referenceerror_new", I64, &[I64]);
    module.declare_function("js_throw_symbol_constructor_type_error", DOUBLE, &[]);
    module.declare_function("js_throw_bigint_constructor_type_error", DOUBLE, &[]);
    module.declare_function("js_throw_strict_eval_arguments_syntax_error", DOUBLE, &[]);
    module.declare_function("js_throw_eval_syntax_error", DOUBLE, &[DOUBLE]);
    module.declare_function(
        "js_throw_restricted_function_property_assignment",
        DOUBLE,
        &[],
    );
    module.declare_function("js_throw_math_constructor_type_error", DOUBLE, &[]);
    module.declare_function("js_throw_json_constructor_type_error", DOUBLE, &[]);
    module.declare_function("js_webcrypto_illegal_constructor", DOUBLE, &[]);
    module.declare_function("js_throw_type_error_const_assignment", DOUBLE, &[DOUBLE]);
    module.declare_function(
        "js_throw_reference_error_unresolvable_assignment",
        DOUBLE,
        &[DOUBLE],
    );
    module.declare_function("js_throw_reference_error_unresolved_get", DOUBLE, &[]);
    // with-statement implicit-global sentinel (HOLE) helpers.
    module.declare_function("js_with_implicit_unset", DOUBLE, &[]);
    module.declare_function("js_with_implicit_read", DOUBLE, &[DOUBLE, DOUBLE]);
    // Iterator-protocol result validation (for-of lazy loop).
    module.declare_function("js_iterator_result_validate", DOUBLE, &[DOUBLE]);
    module.declare_function("js_global_get_or_throw_unresolved", DOUBLE, &[DOUBLE]);
    // Ambient `require` for compiled external / compilePackages modules (#5373):
    // bind a bare `require` to a createRequire-backed closure instead of throwing
    // ReferenceError. Takes no base; returns the require closure value.
    module.declare_function("js_module_ambient_require", DOUBLE, &[]);
    // #5389 Tier 2: synchronous ambient require(spec) resolution — the codegen
    // fallthrough when a computed require() didn't const-fold to a compiled target.
    module.declare_function("js_module_ambient_require_apply", DOUBLE, &[DOUBLE]);
    // Non-throwing global read for `typeof <unresolved>` + global read-modify-
    // write for `i++`/`i--` on a sloppy implicit global (#3575).
    module.declare_function("js_global_get_optional", DOUBLE, &[DOUBLE]);
    module.declare_function("js_global_update", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    // #5989: strict-mode `Date = wrapped` — write an EXISTING global
    // property, throw the spec ReferenceError only when absent.
    module.declare_function(
        "js_global_assign_existing_or_throw",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    module.declare_function("js_throw_reference_error_this_before_super", DOUBLE, &[]);
    module.declare_function("js_throw_reference_error_super_delete", DOUBLE, &[]);
    module.declare_function(
        "js_throw_reference_error_unresolved_assignment",
        DOUBLE,
        &[],
    );
    module.declare_function("js_evalerror_new", I64, &[I64]);
    module.declare_function("js_urierror_new", I64, &[I64]);
    // WeakMap / WeakSet / WeakRef / FinalizationRegistry — called
    // via ExternFuncRef from the HIR lowering (which synthesizes
    // `Call(ExternFuncRef("js_weakmap_set"), [...])`). The f64/f64
    // ABI matches both the runtime signature and the codegen's
    // generic extern-call path at lower_call.rs:149.
    module.declare_function("js_weakmap_new", I64, &[]);
    module.declare_function("js_weakset_new", I64, &[]);
    module.declare_function("js_weakmap_init_iterable", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_weakset_init_iterable", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_weakmap_set", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_weakmap_get", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_weakmap_has", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_weakmap_delete", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_weakset_add", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_weakset_has", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_weakset_delete", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_weak_throw_primitive", DOUBLE, &[]);
    // Buffer.from(str, encoding) runtime helpers.
    module.declare_function("js_buffer_from_string", I64, &[I64, I32]);
    module.declare_function("js_encoding_tag_from_value", I32, &[DOUBLE]);
    // Universal `.toString(encoding)` dispatch — branches on
    // is_registered_buffer at runtime, falls back to js_jsvalue_to_string.
    module.declare_function("js_value_to_string_with_encoding", I64, &[DOUBLE, I32]);
    // Buffer-encoding OR number/bigint-radix dispatch (#2864): the string arg
    // is ambiguous, so pass both the pre-parsed encoding tag and the raw arg.
    module.declare_function(
        "js_value_to_string_with_encoding_or_radix",
        I64,
        &[DOUBLE, I32, DOUBLE],
    );
    module.declare_function("js_fs_unlink_sync", I32, &[DOUBLE]);
    module.declare_function("js_object_values", I64, &[I64]);
    module.declare_function("js_object_values_value", I64, &[DOUBLE]);
    module.declare_function("js_object_entries", I64, &[I64]);
    module.declare_function("js_object_entries_value", I64, &[DOUBLE]);
    module.declare_function("js_path_join", I64, &[I64, I64]);
    module.declare_function("js_path_win32_join", I64, &[I64, I64]);
    // path.win32 sub-namespace (issue #1162)
    module.declare_function("js_path_win32_dirname", I64, &[I64]);
    module.declare_function("js_path_win32_basename", I64, &[I64]);
    module.declare_function("js_path_win32_basename_ext", I64, &[I64, I64]);
    module.declare_function("js_path_win32_extname", I64, &[I64]);
    module.declare_function("js_path_win32_is_absolute", I32, &[I64]);
    module.declare_function("js_path_win32_normalize", I64, &[I64]);
    module.declare_function("js_path_win32_parse", I64, &[I64]);
    module.declare_function("js_path_win32_format", I64, &[DOUBLE]);
    module.declare_function("js_path_win32_relative", I64, &[I64, I64]);
    module.declare_function("js_path_win32_relative_checked", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_path_win32_resolve", I64, &[I64]);
    module.declare_function("js_path_win32_resolve_join", I64, &[I64, I64]);
    module.declare_function("js_path_win32_to_namespaced_path", I64, &[I64]);
    module.declare_function("js_path_win32_to_namespaced_path_value", DOUBLE, &[DOUBLE]);
    module.declare_function("js_path_win32_matches_glob", I32, &[I64, I64]);
    module.declare_function("js_path_win32_sep_get", I64, &[]);
    module.declare_function("js_path_win32_delimiter_get", I64, &[]);
    module.declare_function("js_path_dirname", I64, &[I64]);
    module.declare_function("js_path_resolve", I64, &[I64]);
    module.declare_function("js_path_relative", I64, &[I64, I64]);
    module.declare_function("js_path_relative_checked", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_path_to_namespaced_path", I64, &[I64]);
    module.declare_function("js_path_to_namespaced_path_value", DOUBLE, &[DOUBLE]);
    module.declare_function("js_path_matches_glob", I32, &[I64, I64]);
    module.declare_function("js_path_resolve_join", I64, &[I64, I64]);
    module.declare_function("js_object_from_entries", DOUBLE, &[DOUBLE]);
    module.declare_function("js_string_match", I64, &[I64, I64]);
    module.declare_function("js_string_match_all", I64, &[I64, I64]);
    module.declare_function("js_string_match_all_value", I64, &[I64, DOUBLE]);
    module.declare_function("llvm.log.f64", DOUBLE, &[DOUBLE]);
    module.declare_function("llvm.log2.f64", DOUBLE, &[DOUBLE]);
    module.declare_function("llvm.log10.f64", DOUBLE, &[DOUBLE]);
    module.declare_function("llvm.exp.f64", DOUBLE, &[DOUBLE]);
    module.declare_function("llvm.sin.f64", DOUBLE, &[DOUBLE]);
    module.declare_function("llvm.cos.f64", DOUBLE, &[DOUBLE]);
    module.declare_function("js_path_basename", I64, &[I64]);
    module.declare_function("js_path_basename_ext", I64, &[I64, I64]);
    module.declare_function("js_path_extname", I64, &[I64]);
    module.declare_function("js_path_sep_get", I64, &[]);
    module.declare_function("js_path_delimiter_get", I64, &[]);
    module.declare_function("js_path_parse", I64, &[I64]);
    // JSON.parse returns JSValue (u64) via integer register on ARM64,
    // not f64. Use I64 return + bitcast to avoid ABI mismatch crash.
    module.declare_function("js_json_parse", I64, &[I64]);
    // #4578: ToString(text) for JSON.parse — heap *StringHeader, throws on Symbol.
    module.declare_function("js_json_text_to_string", I64, &[DOUBLE]);
    // #2900: JSON.rawJSON(text) / JSON.isRawJSON(value). Both take and return
    // a NaN-boxed f64 (the wrapper object pointer / a boolean).
    module.declare_function("js_json_raw_json", DOUBLE, &[DOUBLE]);
    module.declare_function("js_json_is_raw_json", DOUBLE, &[DOUBLE]);
    // JSON.parse(text) shim that returns `null` for a null `text_ptr`
    // instead of throwing. Used by NR_OBJ_FROM_JSON_STR dispatch rows
    // (e.g. `jwt.verify` on bad signature) — see issue #927.
    module.declare_function("js_json_parse_or_null", I64, &[I64]);
    // JSON.parse<T[]> schema-directed parse: same return semantics.
    // Args: text_ptr (i64), packed_keys (i64), packed_keys_len (i32),
    // field_count (i32).
    module.declare_function("js_json_parse_typed_array", I64, &[I64, I64, I32, I32]);
    // Date string formatters
    module.declare_function("js_date_to_date_string", I64, &[DOUBLE]);
    module.declare_function("js_date_to_time_string", I64, &[DOUBLE]);
    module.declare_function("js_date_to_utc_string", I64, &[DOUBLE]);
    module.declare_function("js_date_to_locale_date_string", I64, &[DOUBLE]);
    module.declare_function("js_date_to_locale_time_string", I64, &[DOUBLE]);
    module.declare_function("js_date_to_json", I64, &[DOUBLE]);
    declare_phase_b_strings_part2(module);
    declare_phase_b_arrays(module);
}
