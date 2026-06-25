//! Phase B object operations (extracted from runtime_decls.rs).

use super::*;

/// Phase B object operations (basic object literals + property get/set).
///
/// - `js_object_alloc(class_id, field_count) -> *mut ObjectHeader` —
///   allocate with class_id=0 for anonymous object literals. The runtime
///   pre-allocates at least 8 inline slots regardless of field_count
///   (`crates/perry-runtime/src/object.rs:500`) to prevent buffer
///   overflow on later set_field calls.
/// - `js_object_set_field_by_name(obj, key, value)` — set field by string
///   key. Both `obj` and `key` are raw i64 pointers; `value` is a
///   NaN-boxed double.
/// - `js_object_get_field_by_name_f64(obj, key) -> f64` — read field by
///   string key, returning the raw f64 (or the NaN-boxed value for
///   non-number fields — same bit pattern, just interpreted differently).
///
/// Field name strings are sourced from the same StringPool the literal
/// strings use, so `obj.x` and `obj["x"]` and `let s = "x"; obj[s]` all
/// share one allocation per unique key.
///
/// The inline bump allocator now handles most object allocation directly;
/// `js_object_alloc(0, N)` is the fallback for dynamic cases.
pub fn declare_phase_b_objects(module: &mut LlModule) {
    // #5093: sticky runtime flag (i8, 0 = enabled) gating the codegen-inlined
    // class-field shape-guard fast path. The inline guard loads this directly
    // and falls back to the full `js_typed_feedback_class_field_*_guard` call
    // when it is non-zero (descriptors / typed-feedback in use). Defined in
    // perry-runtime as `PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED`.
    module.add_external_global("PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED", I8);
    // #5525 follow-up: the process-global typed-array kind cache + the
    // "any exotic views live" guard, exported from perry-runtime so the codegen
    // can emit a guarded *inline* typed-array element load at the access site
    // (cache probe + bounds check + direct slot load) instead of an out-of-line
    // `js_dyn_index_get` call. The cache is a fixed `[64 x i64]` array of
    // `(addr << 8) | tag` words; the view guard is a single `i64` counter that
    // reads 0 whenever every live typed array uses inline storage (so the
    // inline `header + 16 + idx*elem_size` load matches the runtime `data_ptr`).
    module.add_external_global("PERRY_TA_KIND_CACHE", "[64 x i64]");
    module.add_external_global("PERRY_TA_VIEW_GUARD", I64);
    module.declare_function("js_object_alloc", I64, &[I32, I32]);
    // #3149: `Object(value)` plain-call coercion. Takes & returns a NaN-boxed
    // JSValue (DOUBLE): nullish/primitive -> fresh {}, object passes through.
    module.declare_function("js_object_coerce", DOUBLE, &[DOUBLE]);
    // #1789: stamp a class-expression's heap object as a class object
    // (object_type = OBJECT_TYPE_CLASS) so typeof → "function" and
    // new/instanceof read class_id from it.
    module.declare_function("js_object_mark_class", VOID, &[I64]);
    // Shape-cache-aware variant: pre-populates keys_array via SHAPE_INLINE_CACHE,
    // so subsequent field stores can use index-based set_field (skipping the
    // per-call linear key-search done by js_object_set_field_by_name).
    module.declare_function("js_object_alloc_with_shape", I64, &[I32, I32, PTR, I32]);
    // Index-based field setter (no key lookup). Hot-path target for object
    // literals with statically-known keys; the i-th field directly maps to
    // the i-th packed-keys entry above.
    //
    // Issue #448: third arg is `JSValue` on the runtime side (a
    // `#[repr(transparent)] u64` wrapper). On AArch64 / x86_64 SysV / Win64
    // ABIs, integer and floating-point arguments use disjoint register
    // classes — declaring the slot as DOUBLE put the NaN-boxed value in
    // an FP register while the Rust function read its `value: JSValue`
    // arg from a GP register, so closure pointers stored into generator
    // iter objects (`{ next, return, throw }` literals built via the
    // shape-cache fast path) read back as `0` and the iterator-protocol
    // loop hung forever. Declaring the slot as I64 routes through the
    // same register class the runtime actually reads.
    module.declare_function("js_object_set_field", VOID, &[I64, I32, I64]);
    module.declare_function("js_object_set_unboxed_f64_field", VOID, &[I64, I32, DOUBLE]);
    module.declare_function("js_object_get_unboxed_f64_field", DOUBLE, &[I64, I32]);
    module.declare_function("js_object_set_field_by_name", VOID, &[I64, I64, DOUBLE]);
    module.declare_function(
        "js_object_set_field_by_name_nonenum",
        VOID,
        &[I64, I64, DOUBLE],
    );
    // #5127: apply ES2022 `cause` from a `super(message, options)` forward to
    // a user Error-subclass instance (a generic object). (this_handle, options)
    module.declare_function("js_error_apply_cause_to_object", VOID, &[I64, DOUBLE]);
    module.declare_function("js_with_has_binding", I32, &[DOUBLE, I64]);
    module.declare_function("js_with_get_binding", DOUBLE, &[DOUBLE, I64]);
    module.declare_function("js_with_set_binding", DOUBLE, &[DOUBLE, I64, DOUBLE, I32]);
    module.declare_function("js_with_delete_binding", I32, &[DOUBLE, I64]);
    module.declare_function("js_pod_scalar_write_compatible", I32, &[DOUBLE, I32]);
    module.declare_function(
        "js_typed_feedback_register_site",
        VOID,
        &[
            I64, I32, PTR, I64, PTR, I64, PTR, I64, PTR, I64, PTR, I64, PTR, I64,
        ],
    );
    module.declare_function("js_typed_feedback_record_guard_pass", VOID, &[I64]);
    module.declare_function("js_typed_feedback_record_guard_fail", VOID, &[I64]);
    module.declare_function("js_typed_feedback_record_fallback_call", VOID, &[I64]);
    module.declare_function(
        "js_typed_feedback_observe_property_get",
        VOID,
        &[I64, I64, I64],
    );
    module.declare_function(
        "js_typed_feedback_observe_property_set",
        VOID,
        &[I64, I64, I64],
    );
    module.declare_function(
        "js_typed_feedback_object_get_field_by_name_f64",
        DOUBLE,
        &[I64, I64, I64],
    );
    module.declare_function(
        "js_typed_feedback_object_set_field_by_name",
        VOID,
        &[I64, I64, I64, DOUBLE],
    );
    module.declare_function(
        "js_typed_feedback_object_set_field_by_name_fast",
        VOID,
        &[I64, I64, I64, DOUBLE],
    );
    module.declare_function(
        "js_typed_feedback_class_field_set_guard",
        I32,
        &[I64, DOUBLE, I32, I64, I64, I32, DOUBLE, I32],
    );
    // #5334 lever A: class-field-SET guard-MISS fallback, outlined. The cold arm
    // of the default diamond collapses from two calls (record_fallback +
    // set_field_by_name) to this one. Args: (site_id, obj_bits, key_raw, value).
    module.declare_function(
        "js_class_field_set_fallback",
        VOID,
        &[I64, I64, I64, DOUBLE],
    );
    // #5334 lever B: class-field-SET inline cache, FULLY outlined. For oversized
    // modules the whole diamond (guard + fast store + fallback) collapses to one
    // call. Args: (site_id, recv, expected_class_id, expected_keys, key,
    // field_index, value, require_raw_f64). Same signature as the set guard.
    module.declare_function(
        "js_class_field_set_ic",
        VOID,
        &[I64, DOUBLE, I32, I64, I64, I32, DOUBLE, I32],
    );
    module.declare_function(
        "js_typed_feedback_class_field_get_guard",
        I32,
        &[I64, DOUBLE, I32, I64, I64, I32, I32],
    );
    // #5391 path 2: class-field-GET inline cache, FULLY outlined. For oversized
    // modules the whole get diamond collapses to one call returning the field
    // value. Args: (site_id, recv, expected_class_id, expected_keys, key,
    // field_index, require_raw_f64). Same signature as the get guard (+ f64 ret).
    module.declare_function(
        "js_class_field_get_ic",
        DOUBLE,
        &[I64, DOUBLE, I32, I64, I64, I32, I32],
    );
    module.declare_function(
        "js_typed_feedback_native_call_method",
        DOUBLE,
        &[I64, DOUBLE, PTR, I64, PTR, I64],
    );
    module.declare_function(
        "js_typed_feedback_native_call_method_apply",
        DOUBLE,
        &[I64, DOUBLE, PTR, I64, I64],
    );
    module.declare_function(
        "js_typed_feedback_method_direct_call_guard",
        I32,
        &[I64, DOUBLE, I32, I64, PTR, I64, PTR],
    );
    module.declare_function("js_method_direct_shape_guard", I32, &[DOUBLE, I32, I64]);
    module.declare_function(
        "js_typed_feedback_closure_direct_call_guard",
        I32,
        &[I64, DOUBLE, PTR, I32, I32],
    );
    module.declare_function(
        "js_typed_feedback_object_set_unboxed_f64_field",
        VOID,
        &[I64, I64, I32, I64, DOUBLE],
    );
    module.declare_function(
        "js_typed_feedback_observe_helper_return",
        DOUBLE,
        &[I64, DOUBLE],
    );
    // Closes #471: polymorphic numeric-key set/get used by the IndexSet/Get
    // fallback when the receiver type isn't statically narrowed to an array.
    // Dispatches by GC type to either the array setter/getter (preserving
    // forwarding-chain follow + lazy-array materialize) or the object
    // setter/getter (after stringifying the index).
    module.declare_function(
        "js_object_set_index_polymorphic",
        VOID,
        &[I64, DOUBLE, DOUBLE],
    );
    module.declare_function("js_object_get_index_polymorphic", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_object_get_field_by_name_f64", DOUBLE, &[I64, I64]);
    // Issue #649: PropertyGet on `NativeModuleRef("fs"/"os"/"crypto"/...)`
    // routes through this — codegen passes (module_name, property_name)
    // and the runtime returns the constant value (or a sub-namespace
    // ObjectHeader for `.constants`-style chains).
    module.declare_function(
        "js_native_module_property_by_name",
        DOUBLE,
        &[PTR, I64, PTR, I64],
    );
    // Issue #894: materialize a NATIVE_MODULE_CLASS_ID-tagged namespace
    // object for `Expr::NativeModuleRef` when it reaches the value-form
    // fallback path (the require-call-result-then-member-access shape
    // produced by `compilePackages` CJS wrapping). Pre-fix the value
    // lowered to `0.0` and any subsequent member access returned
    // undefined, tripping the spec property-access throw.
    module.declare_function("js_create_native_module_namespace", DOUBLE, &[PTR, I64]);
    // Per-module native-module dispatch-install symbols (devirt). Codegen emits
    // the matching one at each js_create_native_module_namespace site.
    module.declare_function("js_nm_install_assert", VOID, &[]);
    module.declare_function("js_nm_install_async_hooks", VOID, &[]);
    module.declare_function("js_nm_install_bigint", VOID, &[]);
    module.declare_function("js_nm_install_buffer", VOID, &[]);
    module.declare_function("js_nm_install_child_process", VOID, &[]);
    module.declare_function("js_nm_install_cluster", VOID, &[]);
    module.declare_function("js_nm_install_console", VOID, &[]);
    module.declare_function("js_nm_install_crypto", VOID, &[]);
    module.declare_function("js_nm_install_dgram", VOID, &[]);
    module.declare_function("js_nm_install_dns", VOID, &[]);
    module.declare_function("js_nm_install_domain", VOID, &[]);
    module.declare_function("js_nm_install_events", VOID, &[]);
    module.declare_function("js_nm_install_fs", VOID, &[]);
    module.declare_function("js_nm_install_http", VOID, &[]);
    module.declare_function("js_nm_install_inspector", VOID, &[]);
    module.declare_function("js_nm_install_module", VOID, &[]);
    module.declare_function("js_nm_install_net", VOID, &[]);
    module.declare_function("js_nm_install_os", VOID, &[]);
    module.declare_function("js_nm_install_path", VOID, &[]);
    module.declare_function("js_nm_install_perf", VOID, &[]);
    module.declare_function("js_nm_install_process", VOID, &[]);
    module.declare_function("js_nm_install_punycode", VOID, &[]);
    module.declare_function("js_nm_install_querystring", VOID, &[]);
    module.declare_function("js_nm_install_readline", VOID, &[]);
    module.declare_function("js_nm_install_repl", VOID, &[]);
    module.declare_function("js_nm_install_sea", VOID, &[]);
    module.declare_function("js_nm_install_sqlite", VOID, &[]);
    module.declare_function("js_nm_install_stream", VOID, &[]);
    module.declare_function("js_nm_install_timers", VOID, &[]);
    module.declare_function("js_nm_install_tls", VOID, &[]);
    module.declare_function("js_nm_install_tty", VOID, &[]);
    module.declare_function("js_nm_install_url", VOID, &[]);
    module.declare_function("js_nm_install_util", VOID, &[]);
    module.declare_function("js_nm_install_v8", VOID, &[]);
    module.declare_function("js_nm_install_vm", VOID, &[]);
    module.declare_function("js_nm_install_wasi", VOID, &[]);
    module.declare_function("js_nm_install_zlib", VOID, &[]);
    module.declare_function("js_nm_install_all", VOID, &[]);
    module.declare_function("js_object_get_field_ic_miss", DOUBLE, &[I64, I64, PTR]);
    // #5391 path 3: full-outlined generic property GET. Collapses the inline
    // receiver-routing + monomorphic-IC + feedback + nullish-throw diamond to a
    // single call for oversized modules. Args: (obj_bits, key_handle, site_id,
    // per-site IC cache global) -> field value.
    module.declare_function("js_object_get_field_ic", DOUBLE, &[I64, I64, I64, PTR]);
    // Object rest destructuring: copy all properties from src except excluded keys.
    // Takes a src object ptr and an array of NaN-boxed strings (the excluded keys),
    // returns a new object pointer.
    module.declare_function("js_object_rest", I64, &[I64, I64]);
    // RequireObjectCoercible for object destructuring (throws on null/undefined).
    module.declare_function("js_require_object_coercible", DOUBLE, &[DOUBLE]);
    // Next.js wall 53: runtime `require(absolutePath.json)` disk fallback.
    module.declare_function("js_require_json_disk", DOUBLE, &[DOUBLE]);
    // Next.js wall 54: runtime `require(absolutePath.js)` -> AOT-compiled module.
    module.declare_function("js_register_path_module", VOID, &[DOUBLE, DOUBLE]);
    module.declare_function("js_require_path_module", DOUBLE, &[DOUBLE]);
    // Next.js wall 54 (part 2): register a Deferred module's `__init` address by
    // path so a runtime `require(absolutePath)` can trigger its lazy init.
    module.declare_function("js_register_path_init", VOID, &[PTR, I64, I64]);
    // Array alloc variant that pre-sets length to N (for exclude_keys array filling).
    module.declare_function("js_array_alloc_with_length", I64, &[I32]);
    // Unchecked array set (plain array, no buffer/Set/Map dispatch).
    module.declare_function("js_array_set_f64_unchecked", VOID, &[I64, I32, DOUBLE]);
    module.declare_function("js_typed_feedback_array_get_f64", DOUBLE, &[I64, I64, I32]);
    module.declare_function(
        "js_typed_feedback_plain_array_index_get_guard",
        I32,
        &[I64, DOUBLE, DOUBLE, I32, I32],
    );
    module.declare_function(
        "js_typed_feedback_numeric_array_index_get_guard",
        I32,
        &[I64, DOUBLE, DOUBLE, I32, I32],
    );
    module.declare_function(
        "js_typed_feedback_array_index_get_fallback_boxed",
        DOUBLE,
        &[I64, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_typed_feedback_array_set_f64",
        VOID,
        &[I64, I64, I32, DOUBLE],
    );
    module.declare_function(
        "js_typed_feedback_array_set_f64_extend",
        I64,
        &[I64, I64, I32, DOUBLE],
    );
    module.declare_function(
        "js_typed_feedback_plain_array_index_set_guard",
        I32,
        &[I64, DOUBLE, I32, DOUBLE, I32],
    );
    module.declare_function(
        "js_typed_feedback_numeric_array_index_set_guard",
        I32,
        &[I64, DOUBLE, I32, DOUBLE, I32],
    );
    module.declare_function(
        "js_typed_feedback_numeric_array_push_guard",
        I32,
        &[I64, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_typed_feedback_array_index_set_fallback_boxed",
        DOUBLE,
        &[I64, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_typed_feedback_observe_array_element",
        VOID,
        &[I64, I64, I32],
    );
    module.declare_function(
        "js_typed_feedback_array_set_string_key",
        I64,
        &[I64, I64, I64, DOUBLE],
    );
    module.declare_function(
        "js_typed_feedback_array_set_index_or_string",
        I64,
        &[I64, I64, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_typed_feedback_object_set_index_polymorphic",
        VOID,
        &[I64, I64, DOUBLE, DOUBLE],
    );

    // --- Proxy / Reflect ---
    module.declare_function("js_proxy_new", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_proxy_revocable", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_proxy_revoke", VOID, &[DOUBLE]);
    module.declare_function("js_proxy_is_revoked", I32, &[DOUBLE]);
    module.declare_function("js_proxy_is_proxy", I32, &[DOUBLE]);
    module.declare_function("js_proxy_target", DOUBLE, &[DOUBLE]);
    module.declare_function("js_proxy_get", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_proxy_set", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_proxy_has", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_proxy_delete", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_proxy_apply", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_proxy_construct", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_reflect_construct", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_reflect_get", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_reflect_set", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function(
        "js_put_value_set",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE, I32],
    );
    module.declare_function(
        "js_super_put_value_set",
        DOUBLE,
        &[I32, DOUBLE, DOUBLE, DOUBLE, I32],
    );
    module.declare_function("js_super_accessor_get", DOUBLE, &[I32, DOUBLE, DOUBLE]);
    module.declare_function("js_reflect_has", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_reflect_delete", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_reflect_own_keys", DOUBLE, &[DOUBLE]);
    module.declare_function("js_reflect_apply", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function(
        "js_reflect_define_property",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_reflect_get_own_property_descriptor",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    module.declare_function("js_reflect_get_prototype_of", DOUBLE, &[DOUBLE]);
    module.declare_function("js_reflect_set_prototype_of", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_reflect_is_extensible", DOUBLE, &[DOUBLE]);
    module.declare_function("js_reflect_prevent_extensions", DOUBLE, &[DOUBLE]);
    module.declare_function(
        "js_reflect_define_metadata",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_reflect_get_metadata", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function(
        "js_reflect_get_own_metadata",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_reflect_has_metadata", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function(
        "js_reflect_has_own_metadata",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_reflect_get_metadata_keys", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function(
        "js_reflect_get_own_metadata_keys",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_reflect_delete_metadata",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );

    // #4973: util.inherits-era `http(s).Server.call(this, handler)` + real
    // `socket.setEncoding(enc)`. Declared here (rather than in stdlib_ffi.rs)
    // to keep that file under the 2000-line CI gate. The server-ctor pair
    // lives in perry-runtime (routes through JS_NATIVE_HTTP_DISPATCH) so it
    // links for every program; args are NaN-boxed JSValues.
    let server_ctor_args = &[DOUBLE, DOUBLE, DOUBLE];
    module.declare_function(
        "js_http_server_construct_with_this",
        DOUBLE,
        server_ctor_args,
    );
    module.declare_function(
        "js_https_server_construct_with_this",
        DOUBLE,
        server_ctor_args,
    );
    module.declare_function("js_net_socket_set_encoding", I64, &[I64, I64]);

    declare_stdlib_ffi(module);
    declare_stdlib_ffi_part2(module);
}
