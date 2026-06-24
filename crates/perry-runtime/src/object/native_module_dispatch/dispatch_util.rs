//! Per-module native-module dispatch buckets, relocated from
//! `native_module_dispatch.rs` to keep each file under the size budget
//! (issue #1103 split). Pure relocation — no logic change. The
//! `nm_general_closures!` macro is supplied by the parent module.
use super::*;

#[allow(
    unused_variables,
    unused_mut,
    unused_unsafe,
    clippy::let_and_return,
    clippy::all
)]
pub(crate) unsafe fn nm_dispatch_util(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
    let NmCtx {
        obj,
        args_ptr,
        args_len,
        assert_skip_prototype,
    } = *ctx;
    let _ = (obj, args_ptr, args_len, assert_skip_prototype);
    nm_general_closures!(
        obj,
        args_ptr,
        args_len,
        arg,
        i32_arg,
        bool_to_f64,
        str_to_f64,
        pack_args,
        pack_args_from,
        bool_tag,
        ptr_addr,
        optional_ptr_addr,
        _arg_event_ptr,
        arg_bits,
        _arg_closure_ptr,
        ptr_to_f64,
        typed_kind
    );
    match (module_name, method_name) {
        ("util", "format") => crate::builtins::js_util_format(pack_args()),
        ("util", "formatWithOptions") => {
            let effective = args_len.saturating_sub(1);
            let mut arr = crate::array::js_array_alloc(effective as u32);
            for i in 1..args_len {
                arr = crate::array::js_array_push_f64(arr, arg(i));
            }
            crate::builtins::js_util_format_with_options(arg(0), arr)
        }
        ("util", "inspect") => crate::builtins::js_util_inspect(arg(0), arg(1)),
        ("util", "convertProcessSignalToExitCode") => {
            crate::os::js_util_convert_process_signal_to_exit_code(arg(0))
        }
        // #2514: libuv-style errno → name/message/map helpers.
        ("util", "getSystemErrorName") => crate::util_syserr::js_util_get_system_error_name(arg(0)),
        ("util", "getSystemErrorMessage") => {
            crate::util_syserr::js_util_get_system_error_message(arg(0))
        }
        ("util", "getSystemErrorMap") => crate::util_syserr::js_util_get_system_error_map(),
        ("util", "aborted") => crate::util_abort::js_util_aborted(arg(0), arg(1)),
        ("util", "transferableAbortController") => {
            crate::util_abort::js_util_transferable_abort_controller()
        }
        ("util", "transferableAbortSignal") => {
            crate::util_abort::js_util_transferable_abort_signal(arg(0))
        }
        ("util", "getCallSites") => crate::util_call_sites::js_util_get_call_sites(arg(0), arg(1)),
        // #2514: util.parseEnv(content) → object.
        ("util", "parseEnv") => crate::util_parse_env::js_util_parse_env(arg(0)),
        ("util", "debuglog") | ("util", "debug") => {
            crate::util_debuglog::js_util_debuglog(arg(0), arg(1))
        }
        ("util", "inherits") => crate::util_inherits::js_util_inherits(arg(0), arg(1)),
        ("util", "_extend") => crate::util_mime::js_util_extend(arg(0), arg(1)),
        ("util", "_errnoException") => {
            crate::util_mime::js_util_errno_exception(arg(0), arg(1), arg(2))
        }
        ("util", "_exceptionWithHostPort") => crate::util_mime::js_util_exception_with_host_port(
            arg(0),
            arg(1),
            arg(2),
            arg(3),
            arg(4),
        ),
        ("util", "MIMEType") => crate::util_mime::js_util_mime_type_new(arg(0)),
        ("util", "MIMEParams") => crate::util_mime::js_util_mime_params_new(),
        ("util", "diff") => crate::util_diff::js_util_diff(arg(0), arg(1)),
        ("util", "isArray") => crate::array::js_array_is_array(arg(0)),
        ("util", "isDeepStrictEqual") => {
            crate::builtins::js_util_is_deep_strict_equal(arg(0), arg(1))
        }
        ("util", "stripVTControlCharacters") => {
            crate::builtins::js_util_strip_vt_control_characters(arg(0))
        }
        ("util", "styleText") => crate::util_style_text::js_util_style_text(arg(0), arg(1), arg(2)),
        // #2514: util.toUSVString(value) → string with lone surrogates → U+FFFD.
        ("util", "toUSVString") => crate::util_usv::js_util_to_usv_string(arg(0)),
        ("util", "setTraceSigInt") => crate::util_settracesigint::js_util_set_trace_sig_int(arg(0)),
        ("util", "promisify") => crate::util_promisify::js_util_promisify(arg(0)),
        ("util", "callbackify") => crate::util_promisify::js_util_callbackify(arg(0)),
        ("util", "deprecate") => crate::util_promisify::js_util_deprecate(arg(0), arg(1), arg(2)),
        ("util", "parseArgs") => crate::util_parse_args::js_util_parse_args(arg(0)),
        ("util", "isPromise") => {
            let v = JSValue::from_bits(arg(0).to_bits());
            bool_tag(
                v.is_pointer()
                    && crate::promise::js_is_promise(
                        v.as_pointer::<crate::promise::Promise>() as *mut crate::promise::Promise
                    ) != 0,
            )
        }
        ("util", "isArrayBuffer") => bool_tag(crate::buffer::is_array_buffer(ptr_addr(arg(0)))),
        ("util", "isSharedArrayBuffer") => {
            bool_tag(crate::buffer::is_shared_array_buffer(ptr_addr(arg(0))))
        }
        ("util", "isAnyArrayBuffer") => {
            bool_tag(crate::buffer::is_any_array_buffer(ptr_addr(arg(0))))
        }
        ("util", "isArrayBufferView") => crate::object::js_util_types_is_array_buffer_view(arg(0)),
        ("util", "isTypedArray") => bool_tag(typed_kind(arg(0)).is_some()),
        ("util", "isUint8Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_UINT8))
        }
        ("util", "isInt8Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_INT8))
        }
        ("util", "isInt16Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_INT16))
        }
        ("util", "isUint16Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_UINT16))
        }
        ("util", "isInt32Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_INT32))
        }
        ("util", "isUint32Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_UINT32))
        }
        ("util", "isFloat32Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_FLOAT32))
        }
        ("util", "isFloat64Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_FLOAT64))
        }
        ("util", "isUint8ClampedArray") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_UINT8_CLAMPED))
        }
        ("util", "isMap") => bool_tag(crate::map::is_registered_map(ptr_addr(arg(0)))),
        ("util", "isSet") => bool_tag(crate::set::is_registered_set(ptr_addr(arg(0)))),

        // ── util.types namespace ──
        ("util.types", "isArgumentsObject") => {
            crate::object::js_util_types_is_arguments_object(arg(0))
        }
        ("util.types", "isPromise") => crate::object::js_util_types_is_promise(arg(0)),
        ("util.types", "isBigIntObject") => crate::object::js_util_types_is_big_int_object(arg(0)),
        ("util.types", "isArrayBuffer") => crate::object::js_util_types_is_array_buffer(arg(0)),
        ("util.types", "isSharedArrayBuffer") => {
            crate::object::js_util_types_is_shared_array_buffer(arg(0))
        }
        ("util.types", "isAnyArrayBuffer") => {
            crate::object::js_util_types_is_any_array_buffer(arg(0))
        }
        ("util.types", "isArrayBufferView") => {
            crate::object::js_util_types_is_array_buffer_view(arg(0))
        }
        ("util.types", "isDataView") => crate::object::js_util_types_is_data_view(arg(0)),
        ("util.types", "isTypedArray") => crate::object::js_util_types_is_typed_array(arg(0)),
        ("util.types", "isUint8Array") => crate::object::js_util_types_is_uint8_array(arg(0)),
        ("util.types", "isInt8Array") => crate::object::js_util_types_is_int8_array(arg(0)),
        ("util.types", "isInt16Array") => crate::object::js_util_types_is_int16_array(arg(0)),
        ("util.types", "isUint16Array") => crate::object::js_util_types_is_uint16_array(arg(0)),
        ("util.types", "isInt32Array") => crate::object::js_util_types_is_int32_array(arg(0)),
        ("util.types", "isUint32Array") => crate::object::js_util_types_is_uint32_array(arg(0)),
        ("util.types", "isFloat16Array") => crate::object::js_util_types_is_float16_array(arg(0)),
        ("util.types", "isFloat32Array") => crate::object::js_util_types_is_float32_array(arg(0)),
        ("util.types", "isFloat64Array") => crate::object::js_util_types_is_float64_array(arg(0)),
        ("util.types", "isUint8ClampedArray") => {
            crate::object::js_util_types_is_uint8_clamped_array(arg(0))
        }
        ("util.types", "isBigInt64Array") => {
            crate::object::js_util_types_is_big_int64_array(arg(0))
        }
        ("util.types", "isBigUint64Array") => {
            crate::object::js_util_types_is_big_uint64_array(arg(0))
        }
        ("util.types", "isMap") => crate::object::js_util_types_is_map(arg(0)),
        ("util.types", "isMapIterator") => crate::object::js_util_types_is_map_iterator(arg(0)),
        ("util.types", "isProxy") => crate::object::js_util_types_is_proxy(arg(0)),
        ("util.types", "isExternal") => crate::object::js_util_types_is_external(arg(0)),
        ("util.types", "isModuleNamespaceObject") => {
            crate::object::js_util_types_is_module_namespace_object(arg(0))
        }
        ("util.types", "isSet") => crate::object::js_util_types_is_set(arg(0)),
        ("util.types", "isSetIterator") => crate::object::js_util_types_is_set_iterator(arg(0)),
        ("util.types", "isWeakMap") => crate::object::js_util_types_is_weak_map(arg(0)),
        ("util.types", "isWeakSet") => crate::object::js_util_types_is_weak_set(arg(0)),
        ("util.types", "isDate") => crate::object::js_util_types_is_date(arg(0)),
        ("util.types", "isRegExp") => crate::object::js_util_types_is_reg_exp(arg(0)),
        ("util.types", "isAsyncFunction") => crate::object::js_util_types_is_async_function(arg(0)),
        ("util.types", "isGeneratorFunction") => {
            crate::object::js_util_types_is_generator_function(arg(0))
        }
        ("util.types", "isGeneratorObject") => {
            crate::object::js_util_types_is_generator_object(arg(0))
        }
        ("util.types", "isNativeError") => crate::object::js_util_types_is_native_error(arg(0)),
        ("util.types", "isKeyObject") => crate::object::js_util_types_is_key_object(arg(0)),
        ("util.types", "isCryptoKey") => crate::object::js_util_types_is_crypto_key(arg(0)),
        ("util.types", "isNumberObject") => crate::object::js_util_types_is_number_object(arg(0)),
        ("util.types", "isStringObject") => crate::object::js_util_types_is_string_object(arg(0)),
        ("util.types", "isBooleanObject") => crate::object::js_util_types_is_boolean_object(arg(0)),
        ("util.types", "isSymbolObject") => crate::object::js_util_types_is_symbol_object(arg(0)),
        ("util.types", "isBoxedPrimitive") => {
            crate::object::js_util_types_is_boxed_primitive(arg(0))
        }

        // ── node:util/types direct module ──
        ("util/types", "isArgumentsObject") => {
            crate::object::js_util_types_is_arguments_object(arg(0))
        }
        ("util/types", "isPromise") => crate::object::js_util_types_is_promise(arg(0)),
        ("util/types", "isBigIntObject") => crate::object::js_util_types_is_big_int_object(arg(0)),
        ("util/types", "isArrayBuffer") => crate::object::js_util_types_is_array_buffer(arg(0)),
        ("util/types", "isSharedArrayBuffer") => {
            crate::object::js_util_types_is_shared_array_buffer(arg(0))
        }
        ("util/types", "isAnyArrayBuffer") => {
            crate::object::js_util_types_is_any_array_buffer(arg(0))
        }
        ("util/types", "isArrayBufferView") => {
            crate::object::js_util_types_is_array_buffer_view(arg(0))
        }
        ("util/types", "isDataView") => crate::object::js_util_types_is_data_view(arg(0)),
        ("util/types", "isTypedArray") => crate::object::js_util_types_is_typed_array(arg(0)),
        ("util/types", "isUint8Array") => crate::object::js_util_types_is_uint8_array(arg(0)),
        ("util/types", "isInt8Array") => crate::object::js_util_types_is_int8_array(arg(0)),
        ("util/types", "isInt16Array") => crate::object::js_util_types_is_int16_array(arg(0)),
        ("util/types", "isUint16Array") => crate::object::js_util_types_is_uint16_array(arg(0)),
        ("util/types", "isInt32Array") => crate::object::js_util_types_is_int32_array(arg(0)),
        ("util/types", "isUint32Array") => crate::object::js_util_types_is_uint32_array(arg(0)),
        ("util/types", "isFloat16Array") => crate::object::js_util_types_is_float16_array(arg(0)),
        ("util/types", "isFloat32Array") => crate::object::js_util_types_is_float32_array(arg(0)),
        ("util/types", "isFloat64Array") => crate::object::js_util_types_is_float64_array(arg(0)),
        ("util/types", "isUint8ClampedArray") => {
            crate::object::js_util_types_is_uint8_clamped_array(arg(0))
        }
        ("util/types", "isBigInt64Array") => {
            crate::object::js_util_types_is_big_int64_array(arg(0))
        }
        ("util/types", "isBigUint64Array") => {
            crate::object::js_util_types_is_big_uint64_array(arg(0))
        }
        ("util/types", "isMap") => crate::object::js_util_types_is_map(arg(0)),
        ("util/types", "isMapIterator") => crate::object::js_util_types_is_map_iterator(arg(0)),
        ("util/types", "isProxy") => crate::object::js_util_types_is_proxy(arg(0)),
        ("util/types", "isExternal") => crate::object::js_util_types_is_external(arg(0)),
        ("util/types", "isModuleNamespaceObject") => {
            crate::object::js_util_types_is_module_namespace_object(arg(0))
        }
        ("util/types", "isSet") => crate::object::js_util_types_is_set(arg(0)),
        ("util/types", "isSetIterator") => crate::object::js_util_types_is_set_iterator(arg(0)),
        ("util/types", "isWeakMap") => crate::object::js_util_types_is_weak_map(arg(0)),
        ("util/types", "isWeakSet") => crate::object::js_util_types_is_weak_set(arg(0)),
        ("util/types", "isDate") => crate::object::js_util_types_is_date(arg(0)),
        ("util/types", "isRegExp") => crate::object::js_util_types_is_reg_exp(arg(0)),
        ("util/types", "isAsyncFunction") => crate::object::js_util_types_is_async_function(arg(0)),
        ("util/types", "isGeneratorFunction") => {
            crate::object::js_util_types_is_generator_function(arg(0))
        }
        ("util/types", "isGeneratorObject") => {
            crate::object::js_util_types_is_generator_object(arg(0))
        }
        ("util/types", "isNativeError") => crate::object::js_util_types_is_native_error(arg(0)),
        ("util/types", "isKeyObject") => crate::object::js_util_types_is_key_object(arg(0)),
        ("util/types", "isCryptoKey") => crate::object::js_util_types_is_crypto_key(arg(0)),
        ("util/types", "isNumberObject") => crate::object::js_util_types_is_number_object(arg(0)),
        ("util/types", "isStringObject") => crate::object::js_util_types_is_string_object(arg(0)),
        ("util/types", "isBooleanObject") => crate::object::js_util_types_is_boolean_object(arg(0)),
        ("util/types", "isSymbolObject") => crate::object::js_util_types_is_symbol_object(arg(0)),
        ("util/types", "isBoxedPrimitive") => {
            crate::object::js_util_types_is_boxed_primitive(arg(0))
        }
        // ── url module (module-level functions return NaN-boxed JS values) ──
        _ => f64::from_bits(JSValue::undefined().bits()),
    }
}
