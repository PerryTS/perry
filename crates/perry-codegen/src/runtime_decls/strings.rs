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
    module.declare_function("js_string_slice", I64, &[I64, I32, I32]);
    module.declare_function("js_string_substring", I64, &[I64, I32, I32]);
    module.declare_function("js_string_split", I64, &[I64, I64]);
    module.declare_function("js_string_split_n", I64, &[I64, I64, I32]);
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
    // Keep js_math_pow for now — Math.pow has overflow / NaN
    // semantics that the libm pow doesn't quite match.

    // JSON.stringify (Phase B.15). The 2-arg form is JsonStringifyFull
    // in the HIR (value, type_hint, indent — actually 3 args; we use the
    // simple 2-arg js_json_stringify for now).
    module.declare_function("js_json_stringify", I64, &[DOUBLE, I32]);

    // Map (Phase B.15). The runtime stores keys/values as NaN-boxed doubles.
    // js_map_alloc returns a *mut MapHeader (i64 pointer).
    module.declare_function("js_map_alloc", I64, &[I32]);
    // typeof: returns a string handle ("number"/"string"/"boolean"/"undefined"/"object"/"function")
    module.declare_function("js_value_typeof", I64, &[DOUBLE]);
    module.declare_function("js_string_starts_with", I32, &[I64, I64]);
    module.declare_function("js_string_ends_with", I32, &[I64, I64]);
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
    // - js_closure_set/get_capture_f64(closure, idx, value)
    //     Read/write a captured value (NaN-boxed double) at slot `idx`.
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
    module.declare_function("js_register_closure_arity", VOID, &[PTR, I32]);
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
    // `new assert.AssertionError({...})` — Expr::NewDynamic special-case.
    module.declare_function("js_assert_assertion_error_ctor", DOUBLE, &[DOUBLE]);
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
    module.declare_function("js_map_set", I64, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_map_get", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_map_has", I32, &[I64, DOUBLE]);
    module.declare_function("js_map_delete", I32, &[I64, DOUBLE]);
    module.declare_function("js_object_keys", I64, &[I64]);
    module.declare_function("js_is_finite", DOUBLE, &[DOUBLE]);
    module.declare_function("js_is_undefined_or_bare_nan", I32, &[DOUBLE]);
    module.declare_function("js_math_min_array", DOUBLE, &[I64]);
    module.declare_function("js_math_max_array", DOUBLE, &[I64]);
    module.declare_function("js_string_coerce", I64, &[DOUBLE]);
    module.declare_function("js_array_slice", I64, &[I64, I32, I32]);
    module.declare_function("js_array_shift_f64", DOUBLE, &[I64]);
    module.declare_function("js_set_alloc", I64, &[I32]);
    module.declare_function("js_set_from_array", I64, &[I64]);
    module.declare_function("js_set_from_iterable", I64, &[DOUBLE]);
    module.declare_function("js_map_from_array", I64, &[I64]);
    module.declare_function("js_object_has_property", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_fs_write_file_sync", I32, &[DOUBLE, DOUBLE]);
    // fs.appendFileSync(path, content) — returns i32 status. Issue #226.
    module.declare_function("js_fs_append_file_sync", I32, &[DOUBLE, DOUBLE]);
    module.declare_function("js_fs_exists_sync", I32, &[DOUBLE]);
    // fs.readFileSync(path, encoding) — returns a raw *mut StringHeader i64.
    module.declare_function("js_fs_read_file_sync", I64, &[DOUBLE]);
    // fs.mkdirSync(path) — returns i32 status (1=success).
    module.declare_function("js_fs_mkdir_sync", I32, &[DOUBLE]);
    // fs.unlinkSync(path) — returns i32 status.
    module.declare_function("js_fs_unlink_sync", I32, &[DOUBLE]);
    // fs.readdirSync(path, options) — returns NaN-boxed array of
    // strings, or array of Dirent objects when
    // `options.withFileTypes === true` (issue #631).
    module.declare_function("js_fs_readdir_sync", DOUBLE, &[DOUBLE, DOUBLE]);
    // fs.statSync(path) — returns a NaN-boxed object with isFile/isDirectory/size fields.
    module.declare_function("js_fs_stat_sync", DOUBLE, &[DOUBLE]);
    // fs.renameSync(from, to) — returns i32 status.
    module.declare_function("js_fs_rename_sync", I32, &[DOUBLE, DOUBLE]);
    // fs.copyFileSync(from, to) — returns i32 status.
    module.declare_function("js_fs_copy_file_sync", I32, &[DOUBLE, DOUBLE]);
    // fs.chmodSync(path, mode) — returns i32 status.
    module.declare_function("js_fs_chmod_sync", I32, &[DOUBLE, DOUBLE]);
    // fs.accessSync(path) — returns i32 status (1=ok, 0=error).
    module.declare_function("js_fs_access_sync", I32, &[DOUBLE]);
    // fs.accessSync(path) — Node-compatible variant that throws on
    // failure (via js_throw → setjmp longjmp). Returns NaN-boxed undefined.
    module.declare_function("js_fs_access_sync_throw", DOUBLE, &[DOUBLE]);
    // fs.realpathSync(path) — returns raw *mut StringHeader i64.
    module.declare_function("js_fs_realpath_sync", I64, &[DOUBLE]);
    // fs.mkdtempSync(prefix) — returns raw *mut StringHeader i64.
    module.declare_function("js_fs_mkdtemp_sync", I64, &[DOUBLE]);
    // fs.rmdirSync(path) — returns i32 status.
    module.declare_function("js_fs_rmdir_sync", I32, &[DOUBLE]);
    // fs.rmRecursive(path) — recursive remove; returns i32 (1=ok, 0=fail).
    module.declare_function("js_fs_rm_recursive", I32, &[DOUBLE]);
    // fs.createWriteStream(path) — returns NaN-boxed stream object.
    module.declare_function("js_fs_create_write_stream", DOUBLE, &[DOUBLE]);
    // fs.createReadStream(path[, options]) — returns NaN-boxed stream object.
    module.declare_function("js_fs_create_read_stream", DOUBLE, &[DOUBLE]);
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
    module.declare_function("js_set_add", I64, &[I64, DOUBLE]);
    module.declare_function("js_set_has", I32, &[I64, DOUBLE]);
    module.declare_function("js_set_delete", I32, &[I64, DOUBLE]);
    module.declare_function("js_set_size", I32, &[I64]);
    module.declare_function("js_string_to_lower_case", I64, &[I64]);
    module.declare_function("js_string_to_upper_case", I64, &[I64]);
    module.declare_function("js_string_trim", I64, &[I64]);
    module.declare_function("js_string_trim_start", I64, &[I64]);
    module.declare_function("js_string_trim_end", I64, &[I64]);
    module.declare_function("js_string_char_at", I64, &[I64, I32]);
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
    module.declare_function("js_string_repeat", I64, &[I64, I32]);
    module.declare_function("js_string_replace_string", I64, &[I64, I64, I64]);
    module.declare_function("js_string_replace_all_string", I64, &[I64, I64, I64]);
    module.declare_function("js_string_equals", I32, &[I64, I64]);
    module.declare_function("js_string_compare", I32, &[I64, I64]);
    module.declare_function("js_jsvalue_to_string_radix", I64, &[DOUBLE, I32]);
    module.declare_function("js_math_random", DOUBLE, &[]);
    // WebAssembly host runtime (issue #76). All take/return NaN-boxed
    // doubles (JSValues). Implementations live in
    // `perry-runtime/src/webassembly.rs` and forward to
    // `perry-wasm-host`'s C ABI; the wasmi engine is only linked when
    // the user passes `--enable-wasm-runtime`.
    module.declare_function("js_webassembly_validate", DOUBLE, &[DOUBLE]);
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
    // #1002: native `util.format` / `util.formatWithOptions`. Codegen
    // bundles the call args into a heap array (same shape as
    // js_console_log_spread) and gets a NaN-boxed string back.
    module.declare_function("js_util_format", DOUBLE, &[I64]);
    module.declare_function("js_util_inspect", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_util_is_deep_strict_equal", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_util_strip_vt_control_characters", DOUBLE, &[DOUBLE]);
    module.declare_function("js_boxed_number_new", DOUBLE, &[DOUBLE]);
    module.declare_function("js_boxed_string_new", DOUBLE, &[DOUBLE]);
    module.declare_function("js_boxed_boolean_new", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_promise", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_array_buffer", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_array_buffer_view", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_typed_array", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_uint8_array", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_uint16_array", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_int32_array", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_float64_array", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_map", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_set", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_date", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_reg_exp", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_number_object", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_string_object", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_boolean_object", DOUBLE, &[DOUBLE]);
    module.declare_function("js_util_types_is_boxed_primitive", DOUBLE, &[DOUBLE]);
    module.declare_function("js_getenv", I64, &[I64]);
    module.declare_function("js_getenv_value", DOUBLE, &[I64]);
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
    module.declare_function("js_process_thread_cpu_usage", DOUBLE, &[]);
    module.declare_function("js_process_available_memory", DOUBLE, &[]);
    module.declare_function("js_process_constrained_memory", DOUBLE, &[]);
    module.declare_function("js_process_getuid", DOUBLE, &[]);
    module.declare_function("js_process_geteuid", DOUBLE, &[]);
    module.declare_function("js_process_getgid", DOUBLE, &[]);
    module.declare_function("js_process_getegid", DOUBLE, &[]);
    module.declare_function("js_process_emit_warning", VOID, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_process_cpu_usage", DOUBLE, &[DOUBLE]);
    module.declare_function("js_process_env", DOUBLE, &[]);
    module.declare_function("js_process_hrtime_bigint", DOUBLE, &[]);
    module.declare_function("js_process_chdir", VOID, &[I64]);
    module.declare_function("js_process_kill", VOID, &[DOUBLE, DOUBLE]);
    module.declare_function("js_process_exit", VOID, &[DOUBLE]);
    module.declare_function("js_process_abort", VOID, &[]);
    module.declare_function("js_process_umask", DOUBLE, &[]);
    module.declare_function("js_process_umask_set", DOUBLE, &[DOUBLE]);
    module.declare_function("js_process_on", VOID, &[I64, I64]);
    module.declare_function("js_process_once", VOID, &[I64, I64]);
    module.declare_function("js_process_next_tick", VOID, &[I64]);
    module.declare_function("js_process_stdin", DOUBLE, &[]);
    module.declare_function("js_process_stdout", DOUBLE, &[]);
    module.declare_function("js_process_stderr", DOUBLE, &[]);
    // readline (#347) — Phase 2 raw-mode toggle + stdin event handlers.
    module.declare_function("js_readline_set_raw_mode", DOUBLE, &[DOUBLE]);
    module.declare_function("js_readline_stdin_on", VOID, &[I64, I64]);
    // tty (#347 Phase 3) — isatty + stdout dimensions + resize handler.
    module.declare_function("js_tty_isatty", DOUBLE, &[DOUBLE]);
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
    module.declare_function("js_box_alloc", I64, &[DOUBLE]);
    module.declare_function("js_box_get", DOUBLE, &[I64]);
    module.declare_function("js_box_set", VOID, &[I64, DOUBLE]);
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
    // js_eq takes JSValue (#[repr(transparent)] u64) for both
    // params + return — i64 in the ABI, not double.
    module.declare_function("js_eq", I64, &[I64, I64]);
    module.declare_function("js_loose_eq", I64, &[I64, I64]);
    module.declare_function("js_number_to_fixed", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_string_replace_regex", I64, &[I64, I64, I64]);
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
    module.declare_function("js_date_to_iso_string", I64, &[DOUBLE]);
    module.declare_function("js_date_to_iso_string_or_throw", I64, &[DOUBLE]);
    module.declare_function("js_date_new_from_timestamp", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_new_from_value", DOUBLE, &[DOUBLE]);
    module.declare_function("js_array_indexOf_f64", I32, &[I64, DOUBLE]);
    module.declare_function("js_array_indexOf_jsvalue", I32, &[I64, DOUBLE]);
    module.declare_function("js_array_includes_f64", I32, &[I64, DOUBLE]);
    module.declare_function("js_array_includes_jsvalue", I32, &[I64, DOUBLE]);
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
    // Map/Set forEach: (collection_ptr, callback_nanboxed_f64) -> void
    module.declare_function("js_map_foreach", VOID, &[I64, DOUBLE]);
    module.declare_function("js_set_foreach", VOID, &[I64, DOUBLE]);
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
    module.declare_function("js_parse_int", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_parse_float", DOUBLE, &[I64]);
    module.declare_function("js_array_reduce", DOUBLE, &[I64, I64, I32, DOUBLE]);
    module.declare_function("js_array_reduce_right", DOUBLE, &[I64, I64, I32, DOUBLE]);
    module.declare_function("js_array_sort_default", I64, &[I64]);
    module.declare_function("js_array_reverse", I64, &[I64]);
    module.declare_function("js_array_flat", I64, &[I64]);
    module.declare_function("js_array_flat_depth", I64, &[I64, DOUBLE]);
    module.declare_function("js_array_flatMap", I64, &[I64, I64]);
    module.declare_function("js_array_sort_with_comparator", I64, &[I64, I64]);
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
    module.declare_function("js_regexp_new", I64, &[I64, I64]);
    module.declare_function("js_regexp_test", I32, &[I64, I64]);
    module.declare_function("js_get_string_pointer_unified", I64, &[DOUBLE]);
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
    module.declare_function("js_bigint_from_string", I64, &[PTR, I32]);
    module.declare_function("js_bigint_from_f64", I64, &[DOUBLE]);
    module.declare_function("js_bigint_cmp", I32, &[I64, I64]);
    // Dynamic bigint arithmetic — lowered from `Expr::Binary` when
    // either operand is statically bigint-typed. These unbox, call
    // the raw `js_bigint_<op>`, and re-box with BIGINT_TAG. Also
    // tolerate mixed bigint/int32 operands.
    module.declare_function("js_dynamic_add", DOUBLE, &[DOUBLE, DOUBLE]);
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
    module.declare_function("js_instanceof", DOUBLE, &[DOUBLE, I32]);
    // v0.5.749: dynamic instanceof — `value instanceof type` where type
    // is a runtime expression (function arg holding class ref).
    module.declare_function("js_instanceof_dynamic", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_register_class_extends_error", VOID, &[I32]);
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
    module.declare_function("js_typeerror_new", I64, &[I64]);
    module.declare_function("js_rangeerror_new", I64, &[I64]);
    module.declare_function("js_syntaxerror_new", I64, &[I64]);
    module.declare_function("js_referenceerror_new", I64, &[I64]);
    // WeakMap / WeakSet / WeakRef / FinalizationRegistry — called
    // via ExternFuncRef from the HIR lowering (which synthesizes
    // `Call(ExternFuncRef("js_weakmap_set"), [...])`). The f64/f64
    // ABI matches both the runtime signature and the codegen's
    // generic extern-call path at lower_call.rs:149.
    module.declare_function("js_weakmap_new", I64, &[]);
    module.declare_function("js_weakset_new", I64, &[]);
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
    module.declare_function("js_fs_unlink_sync", I32, &[DOUBLE]);
    module.declare_function("js_object_values", I64, &[I64]);
    module.declare_function("js_object_entries", I64, &[I64]);
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
    module.declare_function("js_path_win32_resolve", I64, &[I64]);
    module.declare_function("js_path_win32_resolve_join", I64, &[I64, I64]);
    module.declare_function("js_path_win32_to_namespaced_path", I64, &[I64]);
    module.declare_function("js_path_win32_matches_glob", I32, &[I64, I64]);
    module.declare_function("js_path_win32_sep_get", I64, &[]);
    module.declare_function("js_path_win32_delimiter_get", I64, &[]);
    module.declare_function("js_path_dirname", I64, &[I64]);
    module.declare_function("js_path_resolve", I64, &[I64]);
    module.declare_function("js_path_relative", I64, &[I64, I64]);
    module.declare_function("js_path_to_namespaced_path", I64, &[I64]);
    module.declare_function("js_path_matches_glob", I32, &[I64, I64]);
    module.declare_function("js_path_resolve_join", I64, &[I64, I64]);
    module.declare_function("js_object_from_entries", DOUBLE, &[DOUBLE]);
    module.declare_function("js_string_match", I64, &[I64, I64]);
    module.declare_function("js_string_match_all", I64, &[I64, I64]);
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
    module.declare_function("js_date_to_locale_date_string", I64, &[DOUBLE]);
    module.declare_function("js_date_to_locale_time_string", I64, &[DOUBLE]);
    module.declare_function("js_date_to_json", I64, &[DOUBLE]);
    // RegExp exec
    module.declare_function("js_regexp_exec", I64, &[I64, I64]);
    module.declare_function("js_number_to_precision", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_number_to_exponential", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_date_new", DOUBLE, &[]);
    module.declare_function("js_number_is_integer", DOUBLE, &[DOUBLE]);
    module.declare_function("js_number_is_nan", DOUBLE, &[DOUBLE]);
    module.declare_function("js_number_is_safe_integer", DOUBLE, &[DOUBLE]);
    // Date parsing / UTC constructors / UTC setters.
    module.declare_function("js_date_parse", DOUBLE, &[I64]);
    module.declare_function(
        "js_date_utc",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    // `new Date(year, month, day?, hour?, min?, sec?, ms?)` (local time).
    module.declare_function(
        "js_date_new_local_components",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_date_set_utc_full_year", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_date_set_utc_month", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_date_set_utc_date", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_date_set_utc_hours", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_date_set_utc_minutes", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_date_set_utc_seconds", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_date_set_utc_milliseconds", DOUBLE, &[DOUBLE, DOUBLE]);
    // Local-time setters (#1187).
    module.declare_function("js_date_set_full_year", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_date_set_month", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_date_set_date", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_date_set_hours", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_date_set_minutes", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_date_set_seconds", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_date_set_milliseconds", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_date_set_time", DOUBLE, &[DOUBLE, DOUBLE]);
    // Math extras (stubs in expr.rs had fallen through to no-op/passthrough).
    module.declare_function("js_math_clz32", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_cbrt", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_fround", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_sinh", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_cosh", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_tanh", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_asinh", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_acosh", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_atanh", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_hypot", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_object_is", DOUBLE, &[DOUBLE, DOUBLE]);
    // Path + URI (wired in expr.rs; runtime already implemented).
    module.declare_function("js_path_normalize", I64, &[I64]);
    module.declare_function("js_path_format", I64, &[DOUBLE]);
    module.declare_function("js_path_is_absolute", I32, &[I64]);
    module.declare_function("js_encode_uri", I64, &[DOUBLE]);
    module.declare_function("js_decode_uri", I64, &[DOUBLE]);
    module.declare_function("js_encode_uri_component", I64, &[DOUBLE]);
    module.declare_function("js_decode_uri_component", I64, &[DOUBLE]);
    // TextEncoder / TextDecoder — LLVM variant uses an ArrayHeader-backed
    // buffer (see `crates/perry-runtime/src/text.rs`). Encode returns an
    // i64 pointing at an ArrayHeader with f64 elements (one per UTF-8
    // byte). Decode accepts both ArrayHeader (from encode) and
    // BufferHeader (from `new Uint8Array([...])`).
    module.declare_function("js_text_encoder_new", I64, &[]);
    module.declare_function("js_text_decoder_new", I64, &[]);
    module.declare_function("js_text_encoder_encode_llvm", I64, &[DOUBLE]);
    module.declare_function("js_text_decoder_decode_llvm", I64, &[DOUBLE]);
    // Microtask queue (queueMicrotask / process.nextTick).
    module.declare_function("js_queue_microtask", VOID, &[I64]);
    module.declare_function("js_queue_next_tick", VOID, &[I64]);
    // #1351: process.nextTick(cb, ...args) — trailing args packed into a
    // stack buffer of doubles, forwarded when the tick fires.
    module.declare_function(
        "js_queue_next_tick_args",
        VOID,
        &[I64, crate::types::PTR, I32],
    );
    module.declare_function("js_drain_queued_microtasks", VOID, &[]);
    // Uint8Array constructor wrapper that flags the resulting buffer so the
    // formatter prints `Uint8Array(N) [ ... ]` instead of `<Buffer ...>`.
    module.declare_function("js_uint8array_from_array", I64, &[I64]);
    // `new Uint8Array(x)` runtime dispatch — handles the non-literal case
    // where `x` could be a number (length) or an array (source data).
    module.declare_function("js_uint8array_new", I64, &[DOUBLE]);
    // Generic typed array runtime (Int8/16/32, Uint16/32, Float32/64, Uint8Clamped).
    // Uint8Array piggybacks on the BufferHeader path.
    module.declare_function("js_typed_array_new_empty", I64, &[I32, I32]);
    module.declare_function("js_typed_array_new_from_array", I64, &[I32, I64]);
    // Runtime-dispatched constructor: handles numeric length OR source-array arg.
    module.declare_function("js_typed_array_new", I64, &[I32, DOUBLE]);
    module.declare_function("js_typed_array_length", I32, &[I64]);
    module.declare_function("js_typed_array_get", DOUBLE, &[I64, I32]);
    module.declare_function("js_typed_array_at", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_typed_array_set", VOID, &[I64, I32, DOUBLE]);
    module.declare_function("js_typed_array_to_reversed", I64, &[I64]);
    module.declare_function("js_typed_array_to_sorted_default", I64, &[I64]);
    module.declare_function("js_typed_array_to_sorted_with_comparator", I64, &[I64, I64]);
    module.declare_function("js_typed_array_with", I64, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_typed_array_find_last", DOUBLE, &[I64, I64]);
    module.declare_function("js_typed_array_find_last_index", DOUBLE, &[I64, I64]);
    // Object introspection / mutation (Agent A's accessor-descriptor work).
    module.declare_function("js_object_has_own", DOUBLE, &[DOUBLE, DOUBLE]);
    // Issue #620: own-property override probe used by class-method dispatch.
    // Returns the stored value if `name` is in obj's own keys_array (data
    // property only — no vtable getter walk), else TAG_UNDEFINED. Lets
    // dispatch detect `this.method = X` overrides at the call site.
    module.declare_function(
        "js_object_get_own_field_or_undef",
        DOUBLE,
        &[DOUBLE, PTR, I64],
    );
    // Issue #629: stub for unresolved namespace imports — returns a stable
    // empty-object pointer so `typeof ns === "object"` and `ns.method`
    // cleanly resolves to undefined (instead of TAG_TRUE → "boolean" /
    // "(boolean).method is not a function").
    module.declare_function("js_unresolved_namespace_stub", DOUBLE, &[]);
    // Issue #841: per-(submodule, export) function-singleton getter for
    // the five Node submodules without perry-stdlib backing. Returns a
    // NaN-boxed ClosureHeader pointer (typeof "function") or TAG_TRUE
    // as a fallback if the (submod_key, name) pair isn't registered.
    module.declare_function(
        "js_node_submodule_export_as_function",
        DOUBLE,
        &[PTR, I32, PTR, I32],
    );
    // Issue #841 companion: per-submodule namespace stub object. Returns
    // a NaN-boxed ObjectHeader pointer whose fields are the function
    // singletons emitted by `js_node_submodule_export_as_function`.
    module.declare_function("js_node_submodule_namespace", DOUBLE, &[PTR, I32]);
    // Issue #692: stub for default-imported callables from unresolved modules —
    // returns NaN-boxed undefined and prints a one-shot diagnostic, so the
    // program links instead of failing with `undefined reference to 'default'`.
    module.declare_function("js_unresolved_default_call", DOUBLE, &[]);
    // Issue #611: real persistent globalThis singleton. Returns a
    // NaN-boxed POINTER to a per-process ObjectHeader so
    // `globalThis[k] = v` then `globalThis[k]` round-trips correctly.
    // The codegen IndexGet/IndexSet paths on `Expr::GlobalGet` route
    // through this helper.
    module.declare_function("js_get_global_this", DOUBLE, &[]);
    module.declare_function("js_global_or_console_property_by_name", DOUBLE, &[I64]);
    // Refs #420: register a static computed-key Symbol field on a class.
    // Called from `init_static_fields` for each `static [Symbol.X] = init`.
    module.declare_function(
        "js_class_register_static_symbol",
        VOID,
        &[I32, DOUBLE, DOUBLE],
    );
    // v0.5.747: register a string-named static field on a class so reads
    // via the runtime dynamic-dispatch path (when the class ref is in an
    // Any-typed local) find the value. Refs #420 / #618 followup.
    module.declare_function(
        "js_class_register_static_field",
        VOID,
        &[I32, PTR, I64, DOUBLE],
    );
    module.declare_function(
        "js_object_define_property",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_object_get_own_property_descriptor",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    module.declare_function("js_object_get_own_property_names", DOUBLE, &[DOUBLE]);
    // Symbol runtime (perry-runtime/src/symbol.rs)
    module.declare_function("js_symbol_new", DOUBLE, &[DOUBLE]);
    module.declare_function("js_symbol_new_empty", DOUBLE, &[]);
    module.declare_function("js_symbol_for", DOUBLE, &[DOUBLE]);
    module.declare_function("js_symbol_key_for", DOUBLE, &[DOUBLE]);
    module.declare_function("js_symbol_description", DOUBLE, &[DOUBLE]);
    module.declare_function("js_symbol_to_string", I64, &[DOUBLE]);
    module.declare_function("js_symbol_equals", I32, &[DOUBLE, DOUBLE]);
    module.declare_function("js_is_symbol", I32, &[DOUBLE]);
    module.declare_function("js_object_get_own_property_symbols", I64, &[DOUBLE]);
    module.declare_function(
        "js_object_set_symbol_property",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_object_get_symbol_property", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_object_create", DOUBLE, &[DOUBLE]);
    module.declare_function("js_object_freeze", DOUBLE, &[DOUBLE]);
    module.declare_function("js_object_seal", DOUBLE, &[DOUBLE]);
    module.declare_function("js_object_prevent_extensions", DOUBLE, &[DOUBLE]);
    // Object spread: copy all own fields from src into dst.
    module.declare_function("js_object_copy_own_fields", VOID, &[I64, DOUBLE]);
    // Object.assign(target, source): mutate target with source's own
    // string-keyed AND symbol-keyed enumerable properties; returns target.
    // Refs #590.
    module.declare_function("js_object_assign_one", DOUBLE, &[DOUBLE, DOUBLE]);
    // String extras (already in string.rs; expr.rs was stubbing or missing dispatch).
    module.declare_function("js_string_at", DOUBLE, &[I64, I32]);
    module.declare_function("js_string_code_point_at", DOUBLE, &[I64, I32]);
    module.declare_function("js_string_from_code_point", I64, &[I32]);
    module.declare_function("js_string_from_char_code", I64, &[I32]);
    module.declare_function("js_string_char_code_at", DOUBLE, &[I64, I32]);
    module.declare_function("js_string_last_index_of", I32, &[I64, I64]);
    module.declare_function("js_string_locale_compare", DOUBLE, &[I64, I64]);
    module.declare_function("js_string_normalize", I64, &[I64, I64]);
    module.declare_function("js_string_pad_start", I64, &[I64, DOUBLE, I64]);
    module.declare_function("js_string_pad_end", I64, &[I64, DOUBLE, I64]);
    module.declare_function("js_string_is_well_formed", DOUBLE, &[I64]);
    module.declare_function("js_string_to_well_formed", I64, &[I64]);
    module.declare_function("js_string_match_all", I64, &[I64, I64]);
    module.declare_function("js_string_search_regex", I32, &[I64, I64]);
    // Regex extras (runtime has them; codegen was stubbing).
    module.declare_function("js_regexp_exec_get_index", DOUBLE, &[]);
    module.declare_function("js_regexp_exec_get_groups", I64, &[]);
    module.declare_function("js_regexp_get_last_index", DOUBLE, &[I64]);
    module.declare_function("js_regexp_set_last_index", VOID, &[I64, DOUBLE]);
    module.declare_function("js_regexp_get_source", I64, &[I64]);
    module.declare_function("js_regexp_get_flags", I64, &[I64]);
    module.declare_function("js_string_replace_regex_named", I64, &[I64, I64, I64]);
    module.declare_function("js_string_replace_regex_fn", I64, &[I64, I64, DOUBLE]);
    // structuredClone(v) — real deep copy, was stubbed as passthrough.
    module.declare_function("js_structured_clone", DOUBLE, &[DOUBLE]);
    // WeakRef / FinalizationRegistry (weakref.rs). `js_weakref_new` /
    // `js_finreg_new` return raw `*mut ObjectHeader` (i64 pointer, must be
    // POINTER_TAG-boxed at the call site). The deref/register/unregister
    // helpers already return NaN-tagged f64 values.
    module.declare_function("js_weakref_new", I64, &[DOUBLE]);
    module.declare_function("js_weakref_deref", DOUBLE, &[DOUBLE]);
    module.declare_function("js_finreg_new", I64, &[DOUBLE]);
    module.declare_function(
        "js_finreg_register",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_finreg_unregister", DOUBLE, &[DOUBLE, DOUBLE]);
    // atob/btoa: base64 decode/encode. Take a NaN-boxed string (f64),
    // return a raw *const StringHeader (i64, must be STRING_TAG-boxed).
    module.declare_function("js_atob", I64, &[DOUBLE]);
    module.declare_function("js_btoa", I64, &[DOUBLE]);
    module.declare_function("js_object_is_frozen", DOUBLE, &[DOUBLE]);
    module.declare_function("js_object_is_sealed", DOUBLE, &[DOUBLE]);
    module.declare_function("js_object_is_extensible", DOUBLE, &[DOUBLE]);
    // Error subclasses (Agent B's runtime work).
    module.declare_function("js_aggregateerror_new", I64, &[I64, I64]);
    module.declare_function("js_error_new_with_cause", I64, &[I64, DOUBLE]);
    // AggregateError.errors field access — returns raw *ArrayHeader.
    module.declare_function("js_error_get_errors", I64, &[I64]);
    // Crypto stdlib — sha256/md5/hmac/randomBytes/randomUUID used by
    // the expr.rs chain collapse for createHash().update().digest().
    module.declare_function("js_crypto_sha256", I64, &[I64]);
    module.declare_function("js_crypto_sha256_bytes", I64, &[I64]);
    module.declare_function("js_crypto_md5", I64, &[I64]);
    module.declare_function("js_crypto_hmac_sha256", I64, &[I64, I64]);
    module.declare_function("js_crypto_hmac_sha256_bytes", I64, &[I64, I64]);
    module.declare_function(
        "js_crypto_pbkdf2_bytes",
        I64,
        &[I64, I64, DOUBLE, DOUBLE, I64],
    );
    module.declare_function("js_crypto_random_bytes_buffer", I64, &[DOUBLE]);
    module.declare_function("js_crypto_random_uuid", I64, &[]);
    // crypto.randomInt([min,] max) -> number; codegen passes min=0 for the
    // single-arg form. Returns the integer as a plain double.
    module.declare_function("js_crypto_random_int", DOUBLE, &[DOUBLE, DOUBLE]);
    // crypto.timingSafeEqual(a, b) -> boolean (NaN-boxed). Args are unboxed
    // to raw i64 pointers (Buffer / TypedArray / string).
    module.declare_function("js_crypto_timing_safe_equal", DOUBLE, &[I64, I64]);
    // crypto.getHashes() / getCiphers() -> string[]; returns *mut ArrayHeader.
    module.declare_function("js_crypto_get_hashes", I64, &[]);
    module.declare_function("js_crypto_get_ciphers", I64, &[]);
    // `crypto.createSecretKey(key, encoding?)` — returns Uint8Array-marked
    // BufferHeader of the key bytes (jose accepts Uint8Array for HS*).
    module.declare_function("js_crypto_create_secret_key", I64, &[I64]);
    // Web Crypto (issue #561): crypto.subtle.{digest,importKey,sign,verify}.
    // Each takes NaN-boxed JS values as f64 and returns a *mut Promise.
    module.declare_function("js_webcrypto_digest", I64, &[DOUBLE, DOUBLE]);
    module.declare_function(
        "js_webcrypto_import_key",
        I64,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_webcrypto_sign", I64, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function(
        "js_webcrypto_verify",
        I64,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    // AES-GCM encrypt / decrypt (issue #561 follow-up). Same Promise
    // shape as sign/verify; runtime resolves synchronously.
    module.declare_function("js_webcrypto_encrypt", I64, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_webcrypto_decrypt", I64, &[DOUBLE, DOUBLE, DOUBLE]);
    // subtle.generateKey(algorithm, extractable, usages) → Promise<CryptoKey>.
    // Initial implementation covers AES-GCM (128/256-bit) — the shape
    // jose's `generateSecret('A256GCM')` reaches for.
    module.declare_function("js_webcrypto_generate_key", I64, &[DOUBLE, DOUBLE, DOUBLE]);
    // subtle.wrapKey(format, key, wrappingKey, wrapAlgorithm) →
    // Promise<Uint8Array>. Initial implementation covers AES-KW
    // (`{ name: 'AES-KW' }`) plus AES-GCM (`{ name: 'AES-GCM', iv }`).
    // Required by jose's `wrapKey`.
    module.declare_function(
        "js_webcrypto_wrap_key",
        I64,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    // subtle.unwrapKey(format, wrappedKey, unwrappingKey, unwrapAlgorithm,
    //   unwrappedKeyAlgorithm, extractable, usages) → Promise<CryptoKey>.
    module.declare_function(
        "js_webcrypto_unwrap_key",
        I64,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    // `zlib.createBrotliDecompress(options?)` — axios feature-check
    // shim. Returns a registered Buffer-shaped handle (NaN-boxed at
    // the call site).
    module.declare_function("js_zlib_create_brotli_decompress", I64, &[DOUBLE]);
    // crypto.randomFillSync(buf, offset?, size?) → returns the same
    // NaN-boxed buffer with random bytes written in-place.
    module.declare_function(
        "js_crypto_random_fill_sync",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    // Hash-handle form (issue #86): `const h = crypto.createHash(alg);
    // h.update(x); h.digest()`. Returns a NaN-boxed POINTER_TAG handle id;
    // subsequent method dispatch flows through HANDLE_METHOD_DISPATCH.
    module.declare_function("js_crypto_create_hash", DOUBLE, &[I64]);
    // Hmac-handle form (issue #1076): `crypto.createHmac(alg, key).update(d).
    // digest(enc)` when `alg` isn't a literal string the chain-collapse
    // recognizes (`const alg = "sha256"`, for-of bindings, ternaries, etc.).
    // Same handle protocol as `js_crypto_create_hash` — POINTER_TAG box, then
    // HANDLE_METHOD_DISPATCH routes `.update` / `.digest` to `dispatch_hmac`.
    module.declare_function("js_crypto_create_hmac", DOUBLE, &[I64, I64]);
    module.declare_function("js_string_from_bytes", I64, &[I64, I32]);
    module.declare_function("js_string_from_wtf8_bytes", I64, &[I64, I32]);
    // Buffer.alloc(size, fill) — returns raw *mut BufferHeader.
    module.declare_function("js_buffer_alloc", I64, &[I32, I32]);
    module.declare_function("js_buffer_alloc_fill_value", I64, &[I32, DOUBLE, I32]);
    // Issue #579: `new ArrayBuffer(size)` — zero-filled BufferHeader of `size`
    // bytes that subsequent `new Uint8Array(ab)` views ALIAS via shared pointer.
    module.declare_function("js_array_buffer_new", I64, &[I32]);
    // JSON full-featured stringify/parse (replacer + indent + reviver).
    module.declare_function("js_json_stringify_full", I64, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_json_parse_with_reviver", I64, &[I64, I64]);
    module.declare_function("js_array_find", DOUBLE, &[I64, I64]);
    module.declare_function("js_array_findIndex", I32, &[I64, I64]);
    module.declare_function("js_array_find_last", DOUBLE, &[I64, I64]);
    module.declare_function("js_array_find_last_index", I32, &[I64, I64]);
    module.declare_function("js_array_some", DOUBLE, &[I64, I64]);
    module.declare_function("js_array_every", DOUBLE, &[I64, I64]);

    // Phase E: async/await runtime support.
    // Promise polling: state is 0=pending, 1=fulfilled, 2=rejected.
    // The await busy-wait loop polls js_promise_state, calls
    // js_promise_run_microtasks + js_sleep_ms while pending, then
    // pulls the value via js_promise_value (or reason via
    // js_promise_reason on rejection).
    module.declare_function("js_promise_state", I32, &[I64]);
    module.declare_function("js_promise_value", DOUBLE, &[I64]);
    module.declare_function("js_promise_reason", DOUBLE, &[I64]);
    // Safe guard used by `Expr::Await` to detect non-promise
    // operands before unboxing. Takes a NaN-boxed f64, returns
    // 1 if it points at a GC_TYPE_PROMISE allocation else 0.
    module.declare_function("js_value_is_promise", I32, &[DOUBLE]);
    // Issue #586: ECMAScript thenable assimilation for `await`. Takes a
    // NaN-boxed f64; returns either the same value (real Promise / non-
    // thenable) or a fresh wrapper Promise that the await loop polls
    // (when the operand is an object whose class chain has a `.then`
    // method). Caller must call this before the `js_value_is_promise`
    // branch so `await thenable` enters the polling path.
    module.declare_function("js_assimilate_thenable", DOUBLE, &[DOUBLE]);
    module.declare_function("js_promise_run_microtasks", I32, &[]);
    // Drain stdlib's tokio async queue (fetch, DB, etc.). Lives in
    // perry-runtime as a thin function-pointer trampoline so it's
    // safe to call even when perry-stdlib is not linked (no-op).
    module.declare_function("js_run_stdlib_pump", VOID, &[]);
    // Drain perry-jsruntime's V8 promise adapter queue. Also a thin
    // perry-runtime trampoline, so non-jsruntime builds pay only a no-op.
    module.declare_function("js_run_jsruntime_pump", VOID, &[]);
    module.declare_function("js_sleep_ms", VOID, &[DOUBLE]);
    // Issue #84: condvar-backed wait for the event loop / await busy-wait.
    // Replaces fixed-quantum `js_sleep_ms(10.0)` / `js_sleep_ms(1.0)`.
    // Returns immediately when a tokio worker calls js_notify_main_thread()
    // after enqueueing onto a queue the pump drains; otherwise sleeps until
    // the next timer deadline (or 1s safety cap).
    module.declare_function("js_wait_for_event", VOID, &[]);
    module.declare_function("js_throw", VOID, &[DOUBLE]);

    // Exception handling (Phase G): setjmp/longjmp-based try/catch.
    // js_try_push() returns a ptr to a jmp_buf.
    // setjmp(ptr) returns i32 (0 on first call, non-0 after longjmp).
    // js_try_end() pops the try depth (no return value).
    // js_get_exception() returns the thrown NaN-boxed value.
    // js_clear_exception() resets the exception state.
    // js_has_exception() returns i32 (1 if exception is active, 0 otherwise).
    // js_enter_finally() / js_leave_finally() bracket finally blocks.
    module.declare_function("js_try_push", PTR, &[]);
    // setjmp variant selection:
    //   - Windows MSVC requires _setjmp(buf, frame_ptr)
    //   - Apple targets: the default C `setjmp(3)` saves the signal mask
    //     via a `sigprocmask` syscall (and the alt-signal-stack via
    //     `__sigaltstack`) — together those syscalls dominate CPU for
    //     async-heavy workloads (~43% of CPU on promise_all_chains.ts
    //     before the swap). Perry never longjmps out of a signal
    //     handler, so the fast `_setjmp(3)` (no sigprocmask) is
    //     functionally equivalent for our exception path. The LLVM-IR
    //     name `_setjmp` maps to the Mach-O linker symbol `__setjmp`
    //     (the C ABI prepends an underscore), which is the fast
    //     variant in libsystem_platform.dylib.
    //   - Linux glibc: the C `setjmp(3)` already does NOT save the
    //     signal mask (POSIX leaves it implementation-defined;
    //     `sigsetjmp(env, 1)` is the signal-saving variant on Linux).
    //     So `setjmp` on Linux is already the fast path; no swap
    //     needed.
    if cfg!(target_os = "windows") {
        module.declare_function("_setjmp", I32, &[PTR, PTR]);
    } else if cfg!(target_vendor = "apple") {
        module.declare_function("_setjmp", I32, &[PTR]);
    } else {
        module.declare_function("setjmp", I32, &[PTR]);
    }
    module.declare_function("js_try_end", VOID, &[]);
    module.declare_function("js_get_exception", DOUBLE, &[]);
    module.declare_function("js_clear_exception", VOID, &[]);
    module.declare_function("js_has_exception", I32, &[]);
    module.declare_function("js_enter_finally", VOID, &[]);
    module.declare_function("js_leave_finally", VOID, &[]);
    module.declare_function("js_await_any_promise", DOUBLE, &[DOUBLE]);
    module.declare_function("js_promise_new", I64, &[]);
    module.declare_function("js_promise_new_with_executor", I64, &[I64]);
    // Timer tick functions — called from the Await busy-wait loop so
    // `setTimeout(resolve, N)` inside a Promise executor actually fires.
    module.declare_function("js_timer_tick", I32, &[]);
    module.declare_function("js_callback_timer_tick", I32, &[]);
    module.declare_function("js_interval_timer_tick", I32, &[]);
    // Timer has-pending checks — called from the main event loop to
    // decide whether to keep ticking or exit.
    module.declare_function("js_timer_has_pending", I32, &[]);
    module.declare_function("js_callback_timer_has_pending", I32, &[]);
    module.declare_function("js_interval_timer_has_pending", I32, &[]);
    // Stdlib has-active-handles — returns 1 if WS servers, pending
    // HTTP events, etc. need the loop to keep running.
    module.declare_function("js_stdlib_has_active_handles", I32, &[]);
    // JS runtime has-active-handles — returns 1 if V8 fallback promises are
    // adapted into native Promises and still pending.
    module.declare_function("js_jsruntime_has_active_handles", I32, &[]);
    // #591: returns 1 iff perry-runtime's per-thread microtask
    // TASK_QUEUE has a pending entry. The codegen-emitted event-loop
    // header check ORs this in so the loop doesn't exit between the
    // body iteration that queues a chained `.then` callback and the
    // next body iteration's microtask drain.
    module.declare_function("js_microtasks_pending", I32, &[]);
    module.declare_function("js_set_timeout_callback", I64, &[I64, DOUBLE]);
    // Refs #665: `setTimeout(fn, delay, ...args)` with trailing args. The
    // args are packed into a stack buffer of doubles at the call site and
    // forwarded by index when the timer fires. Used by Promise-executor
    // patterns like `setTimeout(resolve, delay, res)`.
    module.declare_function(
        "js_set_timeout_callback_args",
        I64,
        &[I64, DOUBLE, crate::types::PTR, I32],
    );
    module.declare_function("js_set_immediate_callback", I64, &[I64]);
    module.declare_function(
        "js_set_immediate_callback_args",
        I64,
        &[I64, crate::types::PTR, I32],
    );
    module.declare_function("setInterval", I64, &[I64, DOUBLE]);
    module.declare_function("clearTimeout", VOID, &[I64]);
    module.declare_function("clearInterval", VOID, &[I64]);
    module.declare_function("js_clear_timeout_value", VOID, &[DOUBLE]);
    module.declare_function("js_clear_interval_value", VOID, &[DOUBLE]);
    module.declare_function("js_buffer_from_array", I64, &[I64]);
    module.declare_function("js_buffer_from_arraybuffer_slice", I64, &[I64, I32, I32]);
    module.declare_function("js_buffer_length", I32, &[I64]);
    module.declare_function("js_buffer_get", I32, &[I64, I32]);
    // console.time/count runtime functions.
    module.declare_function("js_console_time", VOID, &[I64]);
    module.declare_function("js_console_time_end", VOID, &[I64]);
    module.declare_function("js_console_time_log", VOID, &[I64]);
    module.declare_function("js_console_time_value", VOID, &[DOUBLE]);
    module.declare_function("js_console_time_end_value", VOID, &[DOUBLE]);
    module.declare_function("js_console_time_log_value", VOID, &[DOUBLE]);
    module.declare_function("js_console_time_log_spread", VOID, &[DOUBLE, I64]);
    module.declare_function("js_console_count", VOID, &[I64]);
    module.declare_function("js_console_count_reset", VOID, &[I64]);
    module.declare_function("js_console_count_value", VOID, &[DOUBLE]);
    module.declare_function("js_console_count_reset_value", VOID, &[DOUBLE]);
    module.declare_function("js_console_group_begin", VOID, &[]);
    module.declare_function("js_console_group_end", VOID, &[]);
    module.declare_function("js_console_clear", VOID, &[]);
    module.declare_function("js_console_noop", VOID, &[]);
    // Universal PropertyGet method dispatch fallback — routes
    // `recv.method(args)` to the runtime's dispatcher when no static
    // codegen path fires. Used by Map/Set methods on plain object fields.
    module.declare_function(
        "js_native_call_method",
        DOUBLE,
        &[DOUBLE, PTR, I64, PTR, I64],
    );
    // Apply form: takes the args as a JS array handle (i64). The runtime
    // materialises the array elements into a temp f64 buffer and forwards to
    // js_native_call_method. Used by `Expr::CallSpread` for the
    // `recv.method(...args)` shape on any-typed receivers.
    module.declare_function(
        "js_native_call_method_apply",
        DOUBLE,
        &[DOUBLE, PTR, I64, I64],
    );
    // v0.5.754: dispatch obj[strKey](args) — computed-key method call.
    // Takes a StringHeader pointer (already-unboxed) for the method name.
    module.declare_function(
        "js_native_call_method_str_key",
        DOUBLE,
        &[DOUBLE, I64, PTR, I64],
    );
    module.declare_function("js_promise_resolve", VOID, &[I64, DOUBLE]);
    module.declare_function("js_promise_reject", VOID, &[I64, DOUBLE]);
    module.declare_function("js_promise_resolved", I64, &[DOUBLE]);
    module.declare_function("js_promise_rejected", I64, &[DOUBLE]);
    // Issue #100: build a module-namespace object from parallel key/
    // value arrays. Called from `__perry_init_<prefix>` (populate the
    // module's `__perry_ns_<prefix>` global) and from `Expr::DynamicImport`
    // (returned wrapped in `js_promise_resolved`). See
    // `crates/perry-runtime/src/object.rs::js_create_namespace`.
    module.declare_function("js_create_namespace", DOUBLE, &[I32, PTR, PTR, PTR]);
    module.declare_function("js_promise_then", I64, &[I64, I64, I64]);
    module.declare_function("js_promise_resolved_then", I64, &[DOUBLE, I64, I64]);
    module.declare_function("js_promise_finally", I64, &[I64, I64]);
    module.declare_function("js_promise_all", I64, &[I64]);
    module.declare_function("js_promise_race", I64, &[I64]);
    module.declare_function("js_promise_any", I64, &[I64]);
    module.declare_function("js_promise_all_settled", I64, &[I64]);
    module.declare_function("js_promise_with_resolvers", I64, &[]);
    module.declare_function("js_array_unshift_f64", I64, &[I64, DOUBLE]);
    module.declare_function("js_array_entries", I64, &[I64]);
    module.declare_function("js_array_keys", I64, &[I64]);
    module.declare_function("js_array_values", I64, &[I64]);

    // ──────────────────────────────────────────────────────────────────
    // Web Fetch API: Response / Headers / Request / Blob constructors +
    // body methods + static factories. These are in
    // `crates/perry-stdlib/src/fetch.rs`. Handles are NaN-boxed POINTER_TAG
    // f64 values (Phase 1 of the handle-NaN-boxing unification) — codegen
    // passes them through as DOUBLE arg kinds without conversion. Untyped
    // property access (`request.url` where `request: any`) routes through
    // `js_object_get_field_by_name`'s strip-tag path → `HANDLE_PROPERTY_DISPATCH`.
    // ──────────────────────────────────────────────────────────────────
    // new Response(body_ptr, status, status_text_ptr, headers_handle) -> f64
    module.declare_function("js_response_new", DOUBLE, &[I64, DOUBLE, I64, DOUBLE]);
    // new Headers() -> f64
    module.declare_function("js_headers_new", DOUBLE, &[]);
    // headers.set(handle_f64, key_ptr, val_ptr) -> f64 (undefined-tag)
    module.declare_function("js_headers_set", DOUBLE, &[DOUBLE, I64, I64]);
    // headers.get(handle_f64, key_ptr) -> *mut StringHeader (i64)
    module.declare_function("js_headers_get", I64, &[DOUBLE, I64]);
    // headers.has(handle_f64, key_ptr) -> f64 (TAG_TRUE/FALSE)
    module.declare_function("js_headers_has", DOUBLE, &[DOUBLE, I64]);
    // headers.delete(handle_f64, key_ptr) -> f64 (undefined-tag)
    module.declare_function("js_headers_delete", DOUBLE, &[DOUBLE, I64]);
    // headers.forEach(handle_f64, cb_nanbox) -> f64 (undefined-tag)
    module.declare_function("js_headers_for_each", DOUBLE, &[DOUBLE, DOUBLE]);
    // headers.keys/values/entries(handle_f64) -> f64 (NaN-boxed POINTER_TAG to ArrayHeader)
    module.declare_function("js_headers_keys", DOUBLE, &[DOUBLE]);
    module.declare_function("js_headers_values", DOUBLE, &[DOUBLE]);
    module.declare_function("js_headers_entries", DOUBLE, &[DOUBLE]);

    // new Request(url_ptr, method_ptr, body_ptr, headers_handle_f64) -> f64
    module.declare_function("js_request_new", DOUBLE, &[I64, I64, I64, DOUBLE]);
    module.declare_function("js_request_get_url", I64, &[DOUBLE]);
    module.declare_function("js_request_get_method", I64, &[DOUBLE]);
    module.declare_function("js_request_get_body", DOUBLE, &[DOUBLE]);

    // Response body getters — handles flow as NaN-boxed POINTER_TAG f64
    // (Phase 1 unification, refs #421). Accessors call `handle_id` to
    // unbox on entry; codegen no longer needs the fptosi conversion.
    module.declare_function("js_fetch_response_status", DOUBLE, &[DOUBLE]);
    module.declare_function("js_fetch_response_status_text", I64, &[DOUBLE]);
    module.declare_function("js_fetch_response_ok", DOUBLE, &[DOUBLE]);
    module.declare_function("js_fetch_response_text", I64, &[DOUBLE]);
    module.declare_function("js_fetch_response_json", I64, &[DOUBLE]);
    // response.headers / .clone() / .arrayBuffer() / .blob() — all take
    // the f64 response handle.
    module.declare_function("js_response_get_headers", DOUBLE, &[DOUBLE]);
    module.declare_function("js_response_clone", DOUBLE, &[DOUBLE]);
    module.declare_function("js_response_array_buffer", I64, &[DOUBLE]);
    module.declare_function("js_response_blob", I64, &[DOUBLE]);
    // Blob instance methods (issue #234) — handle is f64 (registry id).
    // arrayBuffer/bytes/text return a Promise pointer (i64); slice returns a
    // new blob handle as f64.
    module.declare_function("js_blob_size", DOUBLE, &[DOUBLE]);
    module.declare_function("js_blob_type", I64, &[DOUBLE]);
    module.declare_function("js_blob_array_buffer", I64, &[DOUBLE]);
    module.declare_function("js_blob_bytes", I64, &[DOUBLE]);
    module.declare_function("js_blob_text", I64, &[DOUBLE]);
    module.declare_function("js_blob_slice", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE, I64]);
    // Issue #1211: Blob / File constructors + object-URL registry.
    module.declare_function("js_blob_new", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_file_new", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_file_name", I64, &[DOUBLE]);
    module.declare_function("js_file_last_modified", DOUBLE, &[DOUBLE]);
    module.declare_function("js_url_create_object_url", I64, &[DOUBLE]);
    module.declare_function("js_url_revoke_object_url", VOID, &[DOUBLE]);
    module.declare_function("js_buffer_resolve_object_url", DOUBLE, &[DOUBLE]);
    // Static factories.
    module.declare_function("js_response_static_json", DOUBLE, &[DOUBLE]);
    module.declare_function("js_response_static_redirect", DOUBLE, &[I64, DOUBLE]);

    // ──────────────────────────────────────────────────────────────────
    // Web Streams API (issue #237) — perry-stdlib/src/streams.rs +
    // blob.stream() / response.body bridge in perry-stdlib/src/fetch.rs.
    // Handles are numeric registry ids carried as f64; promise-returning
    // FFIs return *mut Promise (I64) which codegen NaN-boxes via
    // nanbox_pointer_inline.
    // ──────────────────────────────────────────────────────────────────
    module.declare_function("js_blob_stream", DOUBLE, &[DOUBLE]);
    module.declare_function("js_response_body", DOUBLE, &[DOUBLE]);
    // ReadableStream constructor + methods.
    module.declare_function(
        "js_readable_stream_new",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_readable_stream_get_reader", DOUBLE, &[DOUBLE]);
    module.declare_function("js_readable_stream_locked", DOUBLE, &[DOUBLE]);
    module.declare_function("js_readable_stream_cancel", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_readable_stream_tee", DOUBLE, &[DOUBLE]);
    module.declare_function("js_readable_stream_pipe_to", I64, &[DOUBLE, DOUBLE]);
    module.declare_function(
        "js_readable_stream_pipe_through",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_readable_stream_controller_enqueue",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    module.declare_function("js_readable_stream_controller_close", DOUBLE, &[DOUBLE]);
    module.declare_function(
        "js_readable_stream_controller_error",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_readable_stream_controller_desired_size",
        DOUBLE,
        &[DOUBLE],
    );
    // ReadableStreamDefaultReader.
    module.declare_function("js_reader_read", I64, &[DOUBLE]);
    module.declare_function("js_reader_release_lock", DOUBLE, &[DOUBLE]);
    module.declare_function("js_reader_closed", I64, &[DOUBLE]);
    module.declare_function("js_reader_cancel", I64, &[DOUBLE, DOUBLE]);
    // WritableStream + Writer.
    module.declare_function(
        "js_writable_stream_new",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_writable_stream_get_writer", DOUBLE, &[DOUBLE]);
    module.declare_function("js_writable_stream_locked", DOUBLE, &[DOUBLE]);
    module.declare_function("js_writable_stream_close", I64, &[DOUBLE]);
    module.declare_function("js_writable_stream_abort", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_writer_write", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_writer_close", I64, &[DOUBLE]);
    module.declare_function("js_writer_abort", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_writer_release_lock", DOUBLE, &[DOUBLE]);
    module.declare_function("js_writer_closed", I64, &[DOUBLE]);
    module.declare_function("js_writer_ready", I64, &[DOUBLE]);
    module.declare_function("js_writer_desired_size", DOUBLE, &[DOUBLE]);
    // TransformStream.
    module.declare_function("js_transform_stream_new", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_transform_stream_readable", DOUBLE, &[DOUBLE]);
    module.declare_function("js_transform_stream_writable", DOUBLE, &[DOUBLE]);
    // Issue #562: stream subclassing (`class X extends WritableStream` etc.).
    // The unwrap helper is wrapped around every stream-FFI receiver so a
    // subclass instance (NaN-boxed object pointer with the registry id
    // stashed under `__perry_stream_handle__`) and a bare numeric
    // handle are interchangeable. The `*_subclass_init` shims are
    // invoked from `Expr::SuperCall` codegen for the three Web Stream
    // base classes.
    module.declare_function("js_stream_unwrap_handle", DOUBLE, &[DOUBLE]);
    module.declare_function(
        "js_readable_stream_subclass_init",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_writable_stream_subclass_init",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_transform_stream_subclass_init",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );

    // ──────────────────────────────────────────────────────────────────
    // AbortController / AbortSignal — perry-runtime/src/url.rs.
    // Returns *mut ObjectHeader (i64 pointer) — codegen NaN-boxes with
    // POINTER_TAG so regular property get can read fields.
    // ──────────────────────────────────────────────────────────────────
    module.declare_function("js_abort_controller_new", I64, &[]);
    module.declare_function("js_abort_controller_signal", I64, &[I64]);
    module.declare_function("js_abort_controller_abort", VOID, &[I64]);
    module.declare_function("js_abort_controller_abort_reason", VOID, &[I64, DOUBLE]);
    module.declare_function("js_abort_signal_add_listener", VOID, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_abort_signal_timeout", I64, &[DOUBLE]);

    declare_phase_b_arrays(module);
}
