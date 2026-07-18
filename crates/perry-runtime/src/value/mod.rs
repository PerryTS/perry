//! JSValue representation using NaN-boxing
//!
//! NaN-boxing is a technique that encodes type information and values
//! in a 64-bit float. IEEE 754 double-precision floats have a specific
//! bit pattern for NaN (Not a Number), and we can use the unused bits
//! in the NaN payload to store pointers or small values.
//!
//! Layout (64 bits):
//! - Regular f64 values (including NaN) are stored directly
//! - Tagged values use a quiet NaN pattern (mantissa bit 51 set), with the
//!   tag in the top 16 bits and the payload in the low 48 bits. Per IEEE
//!   754 §6.2.1, a NaN is quiet iff mantissa bit 51 is set; every tag
//!   prefix Perry uses (0x7FF8..=0x7FFF) has that bit set, so all tagged
//!   values are qNaN. Quiet matters because arithmetic on qNaN propagates
//!   silently (`undefined + 1 -> NaN`) whereas sNaN would trap the FPU.
//!
//! We use the top 16 bits for tagging:
//! - 0x7FF9: short string (SSO, inline 5-byte payload)
//! - 0x7FFA: bigint pointer
//! - 0x7FFB: JS runtime handle
//! - 0x7FFC + tag: singleton specials (undefined / null / true / false / hole)
//! - 0x7FFD: object/array pointer (48-bit payload)
//! - 0x7FFE: int32 (low 32 bits)
//! - 0x7FFF: heap string pointer (48-bit payload)
//! - Other: regular f64 (including canonical qNaN 0x7FF8_0000_0000_0000)
//!
//! Module layout: `tags` holds the bit constants + JS-handle static slots
//! (read by codegen + every dispatcher); `jsvalue` is the typed front;
//! `handle` registers the perry-jsruntime callbacks; `nanbox`,
//! `dyn_index`, `to_string`, `equality`, `truthy`, `dynamic_arith`,
//! `dynamic_array`, and `dynamic_object` are topical FFI helper banks
//! called from generated LLVM IR. `mod.rs` only contains the explicit
//! re-export surface — perry-codegen + perry-runtime consumers
//! pattern-match against the names below.

pub mod addr_class;
mod dyn_index;
mod dynamic_arith;
mod dynamic_array;
mod dynamic_object;
pub(crate) mod equality;
mod handle;
mod jsvalue;
mod nanbox;
mod tags;
pub(crate) mod to_string;
mod truthy;

#[cfg(test)]
mod tests;

// ----- Tag constants (load-bearing for codegen + cross-module match patterns) -----
// `TAG_MARKER` lives in `tags.rs` with `#[allow(dead_code)]` — kept as a
// named constant for documentation + external tooling but not re-exported
// since no in-crate consumer references it directly.
pub(crate) use tags::{
    BIGINT_TAG, INT32_MASK, INT32_TAG, JS_HANDLE_TAG, POINTER_MASK, POINTER_TAG,
    SHORT_STRING_DATA_MASK, SHORT_STRING_LEN_MASK, SHORT_STRING_LEN_SHIFT, SHORT_STRING_TAG,
    STRING_TAG, TAG_FALSE, TAG_HOLE, TAG_MASK, TAG_NULL, TAG_TDZ, TAG_TRUE, TAG_UNDEFINED,
};
pub use tags::{
    JS_HANDLE_CALL_METHOD, JS_HANDLE_TYPEOF, JS_NATIVE_ASYNC_HOOKS_CONSTRUCT,
    JS_NATIVE_CRYPTO_DISPATCH, JS_NATIVE_DOMAIN_DISPATCH, JS_NATIVE_EVENTS_CONSTRUCT,
    JS_NATIVE_HTTP_DISPATCH, JS_NATIVE_MODULE_JS_LOADER, JS_NATIVE_QUERYSTRING_DISPATCH,
    JS_NATIVE_SQLITE_DISPATCH, JS_NATIVE_TLS_DISPATCH, JS_NATIVE_WEBCRYPTO_DISPATCH,
    JS_NATIVE_ZLIB_DISPATCH, JS_NEW_FROM_HANDLE_V8, SHORT_STRING_MAX_LEN,
};

// Crate-internal handle dispatch atomics + callback type aliases (read by
// every dispatcher that needs to call back into perry-jsruntime).
pub(crate) use tags::{
    JsHandleArrayGetFn, JsHandleArrayLengthFn, JsHandleCallMethodFn, JsHandleObjectGetPropertyFn,
    JsHandleToStringFn, JsHandleTypeofFn, JsNativeCryptoDispatchFn, JsNativeDomainDispatchFn,
    JsNativeEventsConstructFn, JsNativeHttpDispatchFn, JsNativeModuleJsLoaderFn,
    JsNativeQuerystringDispatchFn, JsNativeSqliteDispatchFn, JsNativeTlsDispatchFn,
    JsNativeWebCryptoDispatchFn, JsNativeZlibDispatchFn, JsNewFromHandleV8Fn, JS_HANDLE_ARRAY_GET,
    JS_HANDLE_ARRAY_LENGTH, JS_HANDLE_OBJECT_GET_PROPERTY, JS_HANDLE_TO_STRING,
};

// ----- JSValue type + impls -----
pub use jsvalue::JSValue;

// ----- JS handle FFI registration + helpers -----
pub(crate) use handle::js_handle_is_function;
pub use handle::{
    is_js_handle, js_handle_array_get, js_handle_array_length, js_set_handle_array_get,
    js_set_handle_array_length, js_set_handle_call_method, js_set_handle_object_get_property,
    js_set_handle_to_string, js_set_handle_typeof, js_set_native_async_hooks_construct,
    js_set_native_crypto_dispatch, js_set_native_domain_dispatch, js_set_native_events_construct,
    js_set_native_http_dispatch, js_set_native_module_js_loader,
    js_set_native_querystring_dispatch, js_set_native_sqlite_dispatch, js_set_native_tls_dispatch,
    js_set_native_webcrypto_dispatch, js_set_native_zlib_dispatch, js_set_new_from_handle_v8,
    native_module_try_js_property,
};

// ----- Basic NaN-box pack / unpack FFI -----
pub use nanbox::{
    js_checkpoint, js_debug_val, js_get_string_pointer_unified, js_nanbox_bigint,
    js_nanbox_get_bigint, js_nanbox_get_pointer, js_nanbox_get_string_pointer, js_nanbox_is_bigint,
    js_nanbox_is_pointer, js_nanbox_is_string, js_nanbox_pointer, js_nanbox_string,
};

// ----- Dynamic arithmetic dispatch (BigInt vs float) -----
pub use dynamic_arith::{
    js_dynamic_add, js_dynamic_bitand, js_dynamic_bitor, js_dynamic_bitxor, js_dynamic_div,
    js_dynamic_mod, js_dynamic_mul, js_dynamic_neg, js_dynamic_pow, js_dynamic_shl,
    js_dynamic_shr, js_dynamic_string_or_number_add, js_dynamic_sub, js_dynamic_ushr,
    js_numeric_step, js_to_numeric,
};

// ----- Dynamic index get/set + bare-NaN check -----
pub use dyn_index::{js_dyn_index_get, js_dyn_index_set, js_is_undefined_or_bare_nan};

// ----- to-string conversion helpers -----
pub(crate) use to_string::{
    coerce_validate_radix, function_to_primitive_for_add, function_to_string_method_result,
    ordinary_to_primitive_for_toprimitive, ordinary_to_primitive_number_for_add,
    to_primitive_number, OrdinaryToPrimitiveOutcome,
};
pub use to_string::{
    js_ensure_string_ptr, js_jsvalue_to_string, js_jsvalue_to_string_coerce,
    js_jsvalue_to_string_method, js_jsvalue_to_string_radix, js_value_to_str_ptr_for_ffi,
};

// ----- Equality, comparison, SameValueZero, dynamic string equality -----
pub use equality::{
    js_dynamic_string_equals, js_jsvalue_compare, js_jsvalue_equals, js_jsvalue_loose_equals,
    js_jsvalue_same_value_zero,
};

// ----- Truthiness -----
pub use truthy::js_is_truthy;

// ----- Dynamic array dispatchers -----
pub use dynamic_array::{
    js_dynamic_array_find, js_dynamic_array_findIndex, js_dynamic_array_get,
    js_dynamic_array_length,
};

// ----- Dynamic object property / collection method / Object.keys -----
pub use dynamic_object::{
    js_collection_method_dispatch, js_dynamic_object_get_property, js_dynamic_object_keys,
    js_get_property, js_value_length_f64,
};
