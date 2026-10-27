use super::*;

/// The single intrinsic factory switch. Both implicit and explicit new.target
/// paths consume the same already-loaded constructor identity.
pub(super) unsafe fn construct(
    name: BuiltinConstructorIdentity,
    func_value: f64,
    args: &[f64],
) -> Option<f64> {
    let args_ptr = args.as_ptr();
    let args_len = args.len();
    match name {
        BuiltinConstructorIdentity::Map
        | BuiltinConstructorIdentity::Set
        | BuiltinConstructorIdentity::WeakMap
        | BuiltinConstructorIdentity::WeakSet => {
            return Some(collections::construct(name, func_value, args, func_value));
        }

        // Reflective construction of the intrinsic `Function` —
        // `new F(p, body)` through a value, `Reflect.construct(Function, …)`,
        // a spread `new Function(...parts)` — goes to the same from-strings
        // entry the literal `new Function(...)` reaches from codegen
        // (`lower_call/new.rs`). Identified by closure identity, so it holds
        // when `globalThis.Function` is reassigned. Routed here, before the
        // generic path allocates an instance: `args_ptr` is a plain copy of
        // the arguments, not a GC root (#10424).
        BuiltinConstructorIdentity::Function => {
            return Some(super::super::js_function_ctor_from_strings(
                args_ptr, args_len,
            ));
        }
        BuiltinConstructorIdentity::Crypto
        | BuiltinConstructorIdentity::CryptoKey
        | BuiltinConstructorIdentity::SubtleCrypto => {
            if let Some(value) = crate::object::global_this::web_hooks::construct(
                crate::object::global_this::web_hooks::WebGroup::Webcrypto,
                name,
                func_value,
                args,
            ) {
                return Some(value);
            }
        }
        BuiltinConstructorIdentity::Symbol => {
            return Some(crate::error::js_throw_symbol_constructor_type_error());
        }
        BuiltinConstructorIdentity::BigInt => {
            return Some(crate::error::js_throw_bigint_constructor_type_error());
        }
        BuiltinConstructorIdentity::Navigator => {
            return Some(crate::error::js_throw_illegal_constructor_type_error());
        }
        BuiltinConstructorIdentity::Date => {
            if args.is_empty() {
                return Some(crate::date_hook_slots::js_date_new());
            }
            if args.len() == 1 {
                return Some(crate::date_hook_slots::js_date_new_from_value(args[0]));
            }
            let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
            let mut vals = [undefined, undefined, 1.0, 0.0, 0.0, 0.0, 0.0];
            for (i, slot) in vals.iter_mut().enumerate() {
                if i < args.len() {
                    *slot = args[i];
                }
            }
            return Some(crate::date_hook_slots::js_date_new_local_components(
                vals[0], vals[1], vals[2], vals[3], vals[4], vals[5], vals[6],
            ));
        }
        BuiltinConstructorIdentity::Boolean => {
            let value = args
                .first()
                .copied()
                .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
            return Some(crate::builtins::js_boxed_boolean_new(value));
        }
        BuiltinConstructorIdentity::Number => {
            let value = args
                .first()
                .copied()
                .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
            return Some(crate::builtins::js_boxed_number_new(value));
        }
        BuiltinConstructorIdentity::String => {
            let value = args
                .first()
                .copied()
                .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
            return Some(crate::builtins::js_boxed_string_new(
                value,
                (!args.is_empty()) as i32,
            ));
        }
        BuiltinConstructorIdentity::Array => {
            if args.len() == 1 {
                let arr = crate::array::js_array_constructor_single(args[0]);
                return Some(crate::value::js_nanbox_pointer(arr as i64));
            }
            // `new Array(a, b, c)`: array filled with the args.
            let len = args.len() as u32;
            let arr = crate::array::js_array_alloc(len);
            (*arr).length = len;
            for (i, &v) in args.iter().enumerate() {
                crate::array::js_array_set_f64(arr, i as u32, v);
            }
            return Some(crate::value::js_nanbox_pointer(arr as i64));
        }
        BuiltinConstructorIdentity::Object => {
            let value = args
                .first()
                .copied()
                .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
            return Some(crate::object::js_object_coerce(value));
        }
        BuiltinConstructorIdentity::Blob => {
            if let Some(value) = crate::object::global_this::web_hooks::construct(
                crate::object::global_this::web_hooks::WebGroup::Webfetch,
                name,
                func_value,
                args,
            ) {
                return Some(value);
            }
        }
        BuiltinConstructorIdentity::File => {
            if let Some(value) = crate::object::global_this::web_hooks::construct(
                crate::object::global_this::web_hooks::WebGroup::Webfetch,
                name,
                func_value,
                args,
            ) {
                return Some(value);
            }
        }
        // Global builtins reached through a VALUE (alias variable,
        // intrinsic lookup, cross-module re-export) rather than by name.
        //
        // #8223: NOT feature-gated. This arm carried a
        // `#[cfg(feature = "global-webfetch")]` inherited from #7008's
        // web-platform size gating when #7779 moved the arms out — but it
        // dispatches Map/Set/WeakMap/WeakSet/WeakRef/EventTarget/
        // AbortController/TextEncoder/URLSearchParams/DisposableStack,
        // whose factories are all unconditional modules. Auto-optimize
        // builds the runtime with a minimal feature set (a bare test gets
        // `async-runtime` alone), so the arm compiled out and every
        // value-held builtin constructor fell through: `new (Map-as-value)`
        // threw "Constructor Map requires 'new'", an aliased EventTarget
        // had no surface. The prebuilt FULL stdlib (PERRY_SKIP_BUILD fast
        // mode) masked it, which is exactly the fast/full gap-suite mode
        // divergence #8223 documents.
        n if builtin_alias_construct::handles(n) => {
            return Some(builtin_alias_construct::construct(n, args));
        }
        BuiltinConstructorIdentity::Headers => {
            let init = args
                .first()
                .copied()
                .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
            return Some(crate::object::global_this_headers_thunk(
                std::ptr::null(),
                crate::closure::JsThis::UNDEFINED,
                init,
            ));
        }
        BuiltinConstructorIdentity::Request => {
            if let Some(value) = crate::object::global_this::web_hooks::construct(
                crate::object::global_this::web_hooks::WebGroup::Webfetch,
                name,
                func_value,
                args,
            ) {
                return Some(value);
            }
        }
        BuiltinConstructorIdentity::Response => {
            if let Some(value) = crate::object::global_this::web_hooks::construct(
                crate::object::global_this::web_hooks::WebGroup::Webfetch,
                name,
                func_value,
                args,
            ) {
                return Some(value);
            }
        }
        BuiltinConstructorIdentity::Event => {
            let event_type = args
                .first()
                .copied()
                .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
            let options = args
                .get(1)
                .copied()
                .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
            let event = crate::event_target::js_event_new(event_type, options, args.len() as u32);
            return Some(crate::value::js_nanbox_pointer(event as i64));
        }
        BuiltinConstructorIdentity::CustomEvent => {
            let event_type = args
                .first()
                .copied()
                .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
            let options = args
                .get(1)
                .copied()
                .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
            let event =
                crate::event_target::js_custom_event_new(event_type, options, args.len() as u32);
            return Some(crate::value::js_nanbox_pointer(event as i64));
        }
        BuiltinConstructorIdentity::DOMException => {
            let message = args
                .first()
                .copied()
                .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
            let name = args
                .get(1)
                .copied()
                .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
            let exception = crate::event_target::js_dom_exception_new(message, name);
            return Some(crate::value::js_nanbox_pointer(exception as i64));
        }
        // #2889: `new (rebound Error subclass)(msg)` through a global
        // constructor value. Mirrors the bare `new TypeError(msg)`
        // lowering so `const E = TypeError; new E("x")` produces a real
        // error instance with the right `.name`.
        BuiltinConstructorIdentity::Error
        | BuiltinConstructorIdentity::TypeError
        | BuiltinConstructorIdentity::RangeError
        | BuiltinConstructorIdentity::ReferenceError
        | BuiltinConstructorIdentity::SyntaxError
        | BuiltinConstructorIdentity::EvalError
        | BuiltinConstructorIdentity::URIError => {
            let kind = match name {
                BuiltinConstructorIdentity::TypeError => crate::error::ERROR_KIND_TYPE_ERROR,
                BuiltinConstructorIdentity::RangeError => crate::error::ERROR_KIND_RANGE_ERROR,
                BuiltinConstructorIdentity::ReferenceError => {
                    crate::error::ERROR_KIND_REFERENCE_ERROR
                }
                BuiltinConstructorIdentity::SyntaxError => crate::error::ERROR_KIND_SYNTAX_ERROR,
                BuiltinConstructorIdentity::EvalError => crate::error::ERROR_KIND_EVAL_ERROR,
                BuiltinConstructorIdentity::URIError => crate::error::ERROR_KIND_URI_ERROR,
                _ => crate::error::ERROR_KIND_ERROR,
            };
            let message = if args.is_empty() {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            } else {
                args[0]
            };
            // `new E(message, options)`: the `{ cause }` option.
            let error = match args.get(1) {
                Some(&options) => {
                    crate::error::js_error_new_kind_with_options_from_value(kind, message, options)
                }
                None => crate::error::js_error_new_kind_from_value(kind, message),
            };
            return Some(crate::value::js_nanbox_pointer(error as i64));
        }
        // `new (rebound AggregateError)(errors, message?, options?)`.
        BuiltinConstructorIdentity::AggregateError => {
            let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
            let arg = |i: usize| args.get(i).copied().unwrap_or(undefined);
            let error = crate::error::js_aggregateerror_new_full(arg(0), arg(1), arg(2));
            return Some(crate::value::js_nanbox_pointer(error as i64));
        }
        // `new (rebound Proxy)(target, handler)`: the constructor a global
        // value reaches through `Reflect.construct`, `new G.Proxy(...)` and a
        // spread `new Proxy(...args)`.
        BuiltinConstructorIdentity::Proxy => {
            let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
            let arg = |i: usize| args.get(i).copied().unwrap_or(undefined);
            return Some(crate::proxy_hooks::proxy_new(arg(0), arg(1)));
        }
        // #2889: `new (rebound RegExp)(pattern, flags)`.
        #[cfg(feature = "regex-engine")]
        BuiltinConstructorIdentity::RegExp => {
            let pattern = args
                .first()
                .copied()
                .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
            let flags = args
                .get(1)
                .copied()
                .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
            return Some(crate::value::js_nanbox_pointer(
                crate::regex::hooked::construct(pattern, flags) as i64,
            ));
        }
        // #2889: `new (rebound TypedArray)(lengthOrSource)`.
        BuiltinConstructorIdentity::Int8Array
        | BuiltinConstructorIdentity::Uint8Array
        | BuiltinConstructorIdentity::Uint8ClampedArray
        | BuiltinConstructorIdentity::Int16Array
        | BuiltinConstructorIdentity::Uint16Array
        | BuiltinConstructorIdentity::Int32Array
        | BuiltinConstructorIdentity::Uint32Array
        | BuiltinConstructorIdentity::Float16Array
        | BuiltinConstructorIdentity::Float32Array
        | BuiltinConstructorIdentity::Float64Array
        | BuiltinConstructorIdentity::BigInt64Array
        | BuiltinConstructorIdentity::BigUint64Array => {
            let kind = match name {
                BuiltinConstructorIdentity::Int8Array => crate::typedarray::KIND_INT8,
                BuiltinConstructorIdentity::Uint8Array => crate::typedarray::KIND_UINT8,
                BuiltinConstructorIdentity::Uint8ClampedArray => {
                    crate::typedarray::KIND_UINT8_CLAMPED
                }
                BuiltinConstructorIdentity::Int16Array => crate::typedarray::KIND_INT16,
                BuiltinConstructorIdentity::Uint16Array => crate::typedarray::KIND_UINT16,
                BuiltinConstructorIdentity::Int32Array => crate::typedarray::KIND_INT32,
                BuiltinConstructorIdentity::Uint32Array => crate::typedarray::KIND_UINT32,
                BuiltinConstructorIdentity::Float16Array => crate::typedarray::KIND_FLOAT16,
                BuiltinConstructorIdentity::Float32Array => crate::typedarray::KIND_FLOAT32,
                BuiltinConstructorIdentity::Float64Array => crate::typedarray::KIND_FLOAT64,
                BuiltinConstructorIdentity::BigInt64Array => crate::typedarray::KIND_BIGINT64,
                _ => crate::typedarray::KIND_BIGUINT64,
            } as i32;
            let arg0 = if args.is_empty() {
                f64::from_bits(crate::value::JSValue::number(0.0).bits())
            } else {
                args[0]
            };
            // `new TA(buffer, byteOffset, length?)` via a *dynamic* constructor
            // value (e.g. test262's `testWithTypedArrayConstructors`, where
            // `TA` is a variable) must honor the offset/length arguments. The
            // single-arg `js_typed_array_new` path dropped them, so every
            // view built this way reported `byteOffset === 0`. Route the
            // multi-arg form through the view constructor, which records the
            // backing/offset so `.byteOffset` / `.buffer` are correct and the
            // result aliases the buffer (mirrors the literal-name codegen
            // path in `lower_call::builtin`). A non-ArrayBuffer `arg0` falls
            // back to `js_typed_array_new` inside `js_typed_array_view`.
            let ta = if args.len() >= 2 {
                let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
                crate::typedarray_view::js_typed_array_view(
                    kind,
                    arg0,
                    args[1],
                    args.get(2).copied().unwrap_or(undefined),
                )
            } else {
                crate::typedarray::js_typed_array_new(kind, arg0)
            };
            return Some(crate::value::js_nanbox_pointer(ta as i64));
        }
        BuiltinConstructorIdentity::TextEncoderStream => {
            if let Some(value) = crate::object::global_this::web_hooks::construct(
                crate::object::global_this::web_hooks::WebGroup::Text,
                name,
                func_value,
                args,
            ) {
                return Some(value);
            }
        }
        BuiltinConstructorIdentity::TextDecoderStream => {
            if let Some(value) = crate::object::global_this::web_hooks::construct(
                crate::object::global_this::web_hooks::WebGroup::Text,
                name,
                func_value,
                args,
            ) {
                return Some(value);
            }
        }
        BuiltinConstructorIdentity::CompressionStream => {
            let format = args
                .first()
                .copied()
                .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
            validate_web_compression_stream_format(format);
            return Some(text_encoding_stream_new_with_constructor(
                func_value,
                CLASS_ID_COMPRESSION_STREAM,
            ));
        }
        BuiltinConstructorIdentity::DecompressionStream => {
            let format = args
                .first()
                .copied()
                .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
            validate_web_compression_stream_format(format);
            return Some(text_encoding_stream_new_with_constructor(
                func_value,
                CLASS_ID_DECOMPRESSION_STREAM,
            ));
        }
        // #4950 (secondary note): react-reconciler captures the global
        // `AbortController` into a local (`AbortControllerLocal = typeof
        // AbortController !== "undefined" ? AbortController : <shim>`) and
        // constructs through the variable. Without this arm the dynamic
        // `new` fell through and threw "AbortController is not a function".
        BuiltinConstructorIdentity::AbortController => {
            let controller = crate::url::js_abort_controller_new();
            return Some(crate::value::js_nanbox_pointer(controller as i64));
        }
        BuiltinConstructorIdentity::MessageChannel => {
            return Some(crate::messaging_hook_slots::js_message_channel_new());
        }
        BuiltinConstructorIdentity::MessagePort => {
            return Some(crate::messaging_hook_slots::js_message_port_constructor_error());
        }
        BuiltinConstructorIdentity::Storage => {
            return Some(crate::web_storage::storage_constructor_illegal(
                std::ptr::null(),
                crate::closure::JsThis::UNDEFINED,
            ));
        }
        BuiltinConstructorIdentity::BroadcastChannel => {
            let name = args
                .first()
                .copied()
                .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
            return Some(crate::messaging_hook_slots::js_broadcast_channel_new(name));
        }
        BuiltinConstructorIdentity::URL => {
            if let Some(value) = crate::object::global_this::web_hooks::construct(
                crate::object::global_this::web_hooks::WebGroup::Url,
                name,
                func_value,
                args,
            ) {
                return Some(value);
            }
        }
        BuiltinConstructorIdentity::URLSearchParams => {
            if let Some(value) = crate::object::global_this::web_hooks::construct(
                crate::object::global_this::web_hooks::WebGroup::Url,
                name,
                func_value,
                args,
            ) {
                return Some(value);
            }
        }
        BuiltinConstructorIdentity::TextEncoder => {
            if let Some(value) = crate::object::global_this::web_hooks::construct(
                crate::object::global_this::web_hooks::WebGroup::Text,
                name,
                func_value,
                args,
            ) {
                return Some(value);
            }
        }
        // `new P(executor)` where `P` holds the global Promise constructor
        // VALUE (`const P = Promise;` / a polyfill alias). Without this arm
        // the match fell through to the closure fallback, which CALLED
        // `promise_constructor_call_thunk` as a plain function — throwing
        // "Constructor Promise requires 'new'". Route to the same executor
        // construction as the static literal lowering.
        BuiltinConstructorIdentity::Promise => {
            let executor = args
                .first()
                .copied()
                .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
            let bits = executor.to_bits();
            let exec_ptr = if (bits & crate::value::TAG_MASK) == crate::value::POINTER_TAG {
                (bits & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader
            } else {
                // Non-callable executor: `js_promise_new_with_executor`
                // validates and throws the spec TypeError synchronously.
                std::ptr::null()
            };
            let promise = crate::promise::js_promise_new_with_executor(exec_ptr);
            return Some(crate::value::js_nanbox_pointer(promise as i64));
        }
        BuiltinConstructorIdentity::TextDecoder => {
            if let Some(value) = crate::object::global_this::web_hooks::construct(
                crate::object::global_this::web_hooks::WebGroup::Text,
                name,
                func_value,
                args,
            ) {
                return Some(value);
            }
        }
        // `new $ArrayBuffer(n)` / `new $DataView(buf, off?, len?)` where the
        // constructor was obtained as a VALUE (e.g. the bundle reads
        // `IN(globalThis, BuiltinConstructorIdentity::DataView)` into a variable) rather than the
        // syntactic `new DataView(...)` that lower_call/builtin.rs handles.
        // Without these arms the dynamic-construct path falls through to
        // "not a function". Mirror the static lowering exactly.
        BuiltinConstructorIdentity::ArrayBuffer | BuiltinConstructorIdentity::SharedArrayBuffer => {
            let size = args.first().copied().unwrap_or(0.0);
            let buf = if name == BuiltinConstructorIdentity::SharedArrayBuffer {
                crate::buffer::js_shared_array_buffer_new_value(size)
            } else {
                // #10873: the `{ maxByteLength }` options bag, exactly as
                // the static lowering passes it.
                let options = args
                    .get(1)
                    .copied()
                    .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
                crate::buffer::js_array_buffer_new_with_options(size, options)
            };
            return Some(crate::value::js_nanbox_pointer(buf as i64));
        }
        BuiltinConstructorIdentity::DataView => {
            let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
            let value = args.first().copied().unwrap_or(undef);
            let offset = args.get(1).copied().unwrap_or(undef);
            let length = args.get(2).copied().unwrap_or(undef);
            return Some(crate::buffer::js_data_view_new(value, offset, length));
        }
        _ => {}
    }

    None
}

/// The `global-webcrypto` intrinsic factories, installed by `web_hooks`.
pub(in crate::object) unsafe fn web_webcrypto_construct(
    name: BuiltinConstructorIdentity,
    _func_value: f64,
    _args: &[f64],
) -> Option<f64> {
    match name {
        BuiltinConstructorIdentity::Crypto
        | BuiltinConstructorIdentity::CryptoKey
        | BuiltinConstructorIdentity::SubtleCrypto => {
            return Some(crate::object::js_webcrypto_illegal_constructor());
        }
        _ => {}
    }
    None
}

/// The `global-webfetch` intrinsic factories, installed by `web_hooks`.
pub(in crate::object) unsafe fn web_webfetch_construct(
    name: BuiltinConstructorIdentity,
    _func_value: f64,
    args: &[f64],
) -> Option<f64> {
    match name {
        BuiltinConstructorIdentity::Blob => {
            let parts = args
                .first()
                .copied()
                .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
            let options = args
                .get(1)
                .copied()
                .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
            return Some(crate::object::global_this_blob_thunk(
                std::ptr::null(),
                crate::closure::JsThis::UNDEFINED,
                parts,
                options,
            ));
        }
        BuiltinConstructorIdentity::File => {
            let parts = args
                .first()
                .copied()
                .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
            let name = args
                .get(1)
                .copied()
                .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
            let options = args
                .get(2)
                .copied()
                .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
            return Some(crate::object::global_this_file_thunk(
                std::ptr::null(),
                crate::closure::JsThis::UNDEFINED,
                parts,
                name,
                options,
            ));
        }
        BuiltinConstructorIdentity::Request => {
            let input = args
                .first()
                .copied()
                .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
            let init = args
                .get(1)
                .copied()
                .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
            return Some(crate::object::global_this_request_thunk(
                std::ptr::null(),
                crate::closure::JsThis::UNDEFINED,
                input,
                init,
            ));
        }
        BuiltinConstructorIdentity::Response => {
            let body = args
                .first()
                .copied()
                .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
            let init = args
                .get(1)
                .copied()
                .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
            return Some(crate::object::global_this_response_thunk(
                std::ptr::null(),
                crate::closure::JsThis::UNDEFINED,
                body,
                init,
            ));
        }
        _ => {}
    }
    None
}

/// The `global-text` intrinsic factories, installed by `web_hooks`.
pub(in crate::object) unsafe fn web_text_construct(
    name: BuiltinConstructorIdentity,
    func_value: f64,
    args: &[f64],
) -> Option<f64> {
    match name {
        BuiltinConstructorIdentity::TextEncoderStream => {
            return Some(text_encoding_stream_new_with_constructor(
                func_value,
                CLASS_ID_TEXT_ENCODER_STREAM,
            ));
        }
        BuiltinConstructorIdentity::TextDecoderStream => {
            return Some(text_encoding_stream_new_with_constructor(
                func_value,
                CLASS_ID_TEXT_DECODER_STREAM,
            ));
        }
        BuiltinConstructorIdentity::TextEncoder => {
            let encoder = crate::text::js_text_encoder_new();
            return Some(crate::value::js_nanbox_pointer(encoder));
        }
        BuiltinConstructorIdentity::TextDecoder => {
            let label = args
                .first()
                .copied()
                .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
            let options = args
                .get(1)
                .copied()
                .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
            let fatal = text_decoder_bool_option(options, "fatal");
            let ignore_bom = text_decoder_bool_option(options, "ignoreBOM");
            let decoder = crate::text::js_text_decoder_new(label, fatal, ignore_bom);
            return Some(crate::value::js_nanbox_pointer(decoder));
        }
        _ => {}
    }
    None
}

/// The `global-url` intrinsic factories, installed by `web_hooks`.
pub(in crate::object) unsafe fn web_url_construct(
    name: BuiltinConstructorIdentity,
    _func_value: f64,
    args: &[f64],
) -> Option<f64> {
    match name {
        BuiltinConstructorIdentity::URL => {
            let input = args
                .first()
                .copied()
                .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
            let input_ptr = crate::url_hooks::url_coerce_string(input);
            let url = if let Some(base) = args.get(1).copied() {
                let base_ptr = crate::url_hooks::url_coerce_string(base);
                crate::url_hooks::url_new_with_base(input_ptr, base_ptr)
            } else {
                crate::url_hooks::url_new(input_ptr)
            };
            return Some(crate::value::js_nanbox_pointer(url as i64));
        }
        BuiltinConstructorIdentity::URLSearchParams => {
            let params = if let Some(init) = args.first().copied() {
                crate::url_hooks::url_search_params_new_any(init)
            } else {
                crate::url_hooks::url_search_params_new_empty()
            };
            return Some(crate::value::js_nanbox_pointer(params as i64));
        }
        _ => {}
    }
    None
}
