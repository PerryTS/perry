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
    module.declare_function(
        "js_typed_feedback_class_field_get_guard",
        I32,
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
    module.declare_function("js_object_get_field_ic_miss", DOUBLE, &[I64, I64, PTR]);
    // Object rest destructuring: copy all properties from src except excluded keys.
    // Takes a src object ptr and an array of NaN-boxed strings (the excluded keys),
    // returns a new object pointer.
    module.declare_function("js_object_rest", I64, &[I64, I64]);
    // RequireObjectCoercible for object destructuring (throws on null/undefined).
    module.declare_function("js_require_object_coercible", DOUBLE, &[DOUBLE]);
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
        &[I64, DOUBLE, I32, DOUBLE],
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
    module.declare_function("js_reflect_set", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
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

    declare_stdlib_ffi(module);
}
