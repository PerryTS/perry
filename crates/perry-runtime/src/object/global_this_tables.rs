//! Static `globalThis` constructor/function metadata.

use super::global_this::*;

const NOOP_CONSTRUCTOR_INFO: *const crate::closure::JsFunctionInfo =
    crate::fn_info!(global_this_builtin_noop_thunk, 1);
const TYPED_ARRAY_CONSTRUCTOR_INFO: *const crate::closure::JsFunctionInfo =
    crate::fn_info!(typed_array_constructor_call_thunk, 1; with_declared(0));
const CONSTRUCT_ONLY_INFO: *const crate::closure::JsFunctionInfo =
    crate::fn_info!(construct_only_builtin_call_thunk, 0);
const WEBCRYPTO_CONSTRUCTOR_INFO: *const crate::closure::JsFunctionInfo =
    crate::fn_info!(webcrypto_illegal_constructor_thunk, 0);

/// A builtin's own declaration, including the CLASS identity of its instances
/// when its prototype occupies a class identity word. As with a member's
/// `JsFunctionInfo::flags`, installation consumes the declared fact; the
/// public name only labels the property that exposes the builtin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BuiltinInstanceIdentity {
    Class(u32),
    Crypto,
    SubtleCrypto,
    CryptoKey,
}

#[derive(Clone, Copy)]
pub(crate) struct BuiltinConstructorDeclaration {
    pub identity: BuiltinConstructorIdentity,
    pub name: &'static str,
    pub info: &'static crate::closure::JsFunctionInfo,
    pub prototype_class: Option<u32>,
    pub prototype_serial: Option<u64>,
    pub prototype_callable: bool,
    /// A sealed runtime prototype's reserved serial (`builtin_proto::seal_runtime_prototype`),
    /// which a runtime layout descriptor names.
    pub sealed_serial: Option<u64>,
    /// The reserved serial of the prototype's holder of named properties
    /// (`%Array.prototype%`'s `ArrayHeader.named`), which its identity word
    /// names.
    pub named_serial: Option<u64>,
    pub instance_identity: Option<BuiltinInstanceIdentity>,
}

macro_rules! builtin_constructors {
    ($($name:ident => $info:expr $(; class $class:expr)? $(; serial $serial:expr)? $(; sealed $sealed:expr)? $(; instance $instance:expr)? $(; prototype_callable $callable:expr)? $(; named_serial $named:expr)?),* $(,)?) => {
        /// Immutable [[Construct]] identity in bits 23..29 of the function header.
        /// Zero belongs to every ordinary, wrapper and bound function.
        #[repr(u8)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[allow(dead_code)] // `None` is the zero encoding
        pub(crate) enum BuiltinConstructorIdentity { None = 0, $($name,)* }
        impl BuiltinConstructorIdentity {
            #[inline(always)]
            pub(crate) fn from_header(word: u32) -> Option<Self> {
                let id = (word & crate::codegen_abi::CLOSURE_CONSTRUCT_MASK)
                    >> crate::codegen_abi::CLOSURE_CONSTRUCT_SHIFT;
                if id == 0 { return None; }
                // Every discriminant in this range is declared by this macro.
                assert!(id <= GLOBAL_THIS_BUILTIN_CONSTRUCTORS.len() as u32);
                Some(unsafe { std::mem::transmute::<u8, Self>(id as u8) })
            }
            pub(crate) fn declaration(self) -> &'static BuiltinConstructorDeclaration {
                &GLOBAL_THIS_BUILTIN_CONSTRUCTORS[self as usize - 1]
            }
            pub(crate) fn name(self) -> &'static str { self.declaration().name }
        }
        const _: () = assert!((BuiltinConstructorIdentity::Buffer as u32) < 128);
        /// Builtin declarations, in the order captured by shared constructor bodies.
        pub(crate) const GLOBAL_THIS_BUILTIN_CONSTRUCTORS: &[BuiltinConstructorDeclaration] = &[
            $(BuiltinConstructorDeclaration {
                identity: BuiltinConstructorIdentity::$name,
                name: stringify!($name),
                // SAFETY: fn_info! returns its own static immutable record.
                info: unsafe { &*$info },
                prototype_class: builtin_constructors!(@optional $($class)?),
                prototype_serial: builtin_constructors!(@optional $($serial)?),
                prototype_callable: builtin_constructors!(@callable $($callable)?),
                sealed_serial: builtin_constructors!(@optional $($sealed)?),
                named_serial: builtin_constructors!(@optional $($named)?),
                instance_identity: builtin_constructors!(@optional $($instance)?),
            },)*
        ];
    };
    (@callable $callable:expr) => { $callable };
    (@callable) => { false };
    (@optional $class:expr) => { Some($class) };
    (@optional) => { None };
}

// JS built-in constructor declarations exposed on `globalThis`. Pre-populated by
// the singleton init in `js_get_global_this` so libraries that read these
// off the global (lodash's `var Array = context.Array; var arrayProto =
// Array.prototype`, the same `(globalThis as any).X` read shape) see a
// non-undefined backing object. Codegen mirrors this list in
// `perry-codegen/src/expr.rs::is_global_this_builtin_name` to decide when
// `globalThis.<Name>` should route through the singleton instead of the
// legacy `0.0` no-value placeholder.
builtin_constructors! {
    Array => crate::fn_info!(global_this_array_thunk, 1; with_rest(0)); instance BuiltinInstanceIdentity::Class(0xFFFF0024); named_serial crate::closure::shape::INTRINSIC_SERIAL_ARRAY_PROTOTYPE,
    Object => crate::fn_info!(global_this_object_thunk, 1; with_declared(1)); instance BuiltinInstanceIdentity::Class(0xFFFF0050),
    String => crate::fn_info!(global_this_string_thunk, 1; with_declared(1)); instance BuiltinInstanceIdentity::Class(0xFFFF00D1),
    Number => crate::fn_info!(global_this_number_thunk, 1; with_declared(1)); instance BuiltinInstanceIdentity::Class(0xFFFF00D0),
    Boolean => crate::fn_info!(global_this_boolean_thunk, 1; with_declared(1)); instance BuiltinInstanceIdentity::Class(0xFFFF00D2),
    Function => crate::fn_info!(unwind_in_tests global_this_function_call_thunk, 1; with_rest(0)); serial crate::closure::shape::INTRINSIC_SERIAL_FUNCTION; instance BuiltinInstanceIdentity::Class(0xFFFF00F0); prototype_callable true,
    RegExp => crate::fn_info!(regexp_constructor_call_thunk, 2; with_declared(2)); instance BuiltinInstanceIdentity::Class(0xFFFF0021),
    Date => crate::fn_info!(global_this_date_thunk, 1; with_declared(1)); instance BuiltinInstanceIdentity::Class(0xFFFF0020),
    Error => crate::fn_info!(error_constructor_call_thunk, 1; with_declared(1)); instance BuiltinInstanceIdentity::Class(crate::error::CLASS_ID_ERROR),
    TypeError => crate::fn_info!(type_error_constructor_call_thunk, 1; with_declared(1)); instance BuiltinInstanceIdentity::Class(crate::error::CLASS_ID_TYPE_ERROR),
    RangeError => crate::fn_info!(range_error_constructor_call_thunk, 1; with_declared(1)); instance BuiltinInstanceIdentity::Class(crate::error::CLASS_ID_RANGE_ERROR),
    SyntaxError => crate::fn_info!(syntax_error_constructor_call_thunk, 1; with_declared(1)); instance BuiltinInstanceIdentity::Class(crate::error::CLASS_ID_SYNTAX_ERROR),
    ReferenceError => crate::fn_info!(reference_error_constructor_call_thunk, 1; with_declared(1)); instance BuiltinInstanceIdentity::Class(crate::error::CLASS_ID_REFERENCE_ERROR),
    EvalError => crate::fn_info!(eval_error_constructor_call_thunk, 1; with_declared(1)); instance BuiltinInstanceIdentity::Class(crate::error::CLASS_ID_EVAL_ERROR),
    URIError => crate::fn_info!(uri_error_constructor_call_thunk, 1; with_declared(1)); instance BuiltinInstanceIdentity::Class(crate::error::CLASS_ID_URI_ERROR),
    AggregateError => NOOP_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::error::CLASS_ID_AGGREGATE_ERROR),
    Symbol => crate::fn_info!(global_this_symbol_thunk, 1; with_declared(1)); instance BuiltinInstanceIdentity::Class(0xFFFF00D4),
    Promise => crate::fn_info!(promise_constructor_call_thunk, 1); instance BuiltinInstanceIdentity::Class(0xFFFF0027),
    Map => crate::fn_info!(map_constructor_call_thunk, 1); instance BuiltinInstanceIdentity::Class(0xFFFF0022),
    Set => crate::fn_info!(set_constructor_call_thunk, 1); instance BuiltinInstanceIdentity::Class(0xFFFF0023),
    WeakMap => crate::fn_info!(weak_map_constructor_call_thunk, 1); class crate::weakref::CLASS_ID_WEAKMAP; instance BuiltinInstanceIdentity::Class(0xFFFF002C),
    WeakSet => crate::fn_info!(weak_set_constructor_call_thunk, 1); class crate::weakref::CLASS_ID_WEAKSET; instance BuiltinInstanceIdentity::Class(0xFFFF002D),
    WeakRef => crate::fn_info!(weak_ref_constructor_call_thunk, 1); class crate::weakref::CLASS_ID_WEAKREF; instance BuiltinInstanceIdentity::Class(crate::weakref::CLASS_ID_WEAKREF),
    Proxy => NOOP_CONSTRUCTOR_INFO,
    BigInt => crate::fn_info!(global_this_bigint_thunk, 1; with_declared(1)); instance BuiltinInstanceIdentity::Class(0xFFFF00D3),
    Uint8Array => TYPED_ARRAY_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::typedarray::CLASS_ID_UINT8_ARRAY),
    Int8Array => TYPED_ARRAY_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::typedarray::CLASS_ID_INT8_ARRAY),
    Uint16Array => TYPED_ARRAY_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::typedarray::CLASS_ID_UINT16_ARRAY),
    Int16Array => TYPED_ARRAY_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::typedarray::CLASS_ID_INT16_ARRAY),
    Uint32Array => TYPED_ARRAY_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::typedarray::CLASS_ID_UINT32_ARRAY),
    Int32Array => TYPED_ARRAY_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::typedarray::CLASS_ID_INT32_ARRAY),
    Float16Array => TYPED_ARRAY_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::typedarray::CLASS_ID_FLOAT16_ARRAY),
    Float32Array => TYPED_ARRAY_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::typedarray::CLASS_ID_FLOAT32_ARRAY),
    Float64Array => TYPED_ARRAY_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::typedarray::CLASS_ID_FLOAT64_ARRAY),
    Uint8ClampedArray => TYPED_ARRAY_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::typedarray::CLASS_ID_UINT8_CLAMPED_ARRAY),
    BigInt64Array => TYPED_ARRAY_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::typedarray::CLASS_ID_BIGINT64_ARRAY),
    BigUint64Array => TYPED_ARRAY_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::typedarray::CLASS_ID_BIGUINT64_ARRAY),
    ArrayBuffer => CONSTRUCT_ONLY_INFO; instance BuiltinInstanceIdentity::Class(0xFFFF0025),
    SharedArrayBuffer => CONSTRUCT_ONLY_INFO; instance BuiltinInstanceIdentity::Class(0xFFFF002E),
    DataView => CONSTRUCT_ONLY_INFO; instance BuiltinInstanceIdentity::Class(0xFFFF002B),
    TextEncoder => NOOP_CONSTRUCTOR_INFO,
    TextDecoder => NOOP_CONSTRUCTOR_INFO,
    TextEncoderStream => NOOP_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::object::CLASS_ID_TEXT_ENCODER_STREAM),
    TextDecoderStream => NOOP_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::object::CLASS_ID_TEXT_DECODER_STREAM),
    CompressionStream => NOOP_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::object::CLASS_ID_COMPRESSION_STREAM),
    DecompressionStream => NOOP_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::object::CLASS_ID_DECOMPRESSION_STREAM),
    // The three core Web Streams constructors. Perry implements them (codegen
    // lowers `new ReadableStream(…)` and `x instanceof ReadableStream`, and the
    // class has an id), but the NAMES were never registered here or in codegen's
    // globalThis-builtin list — so a bare `ReadableStream` identifier resolved to
    // nothing: `typeof ReadableStream === "undefined"` and `"ReadableStream" in
    // globalThis` was false, whereas Node exposes all three as functions.
    // Libraries feature-detect precisely that (`typeof ReadableStream !==
    // "undefined" ? … : …`) when deciding how to consume a `fetch()` body.
    ReadableStream => NOOP_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(0xFFFF0060),
    WritableStream => NOOP_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(0xFFFF0061),
    TransformStream => NOOP_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(0xFFFF0062),
    Navigator => NOOP_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::navigator::NAVIGATOR_CLASS_ID),
    URL => NOOP_CONSTRUCTOR_INFO; sealed crate::closure::shape::INTRINSIC_SERIAL_URL; instance BuiltinInstanceIdentity::Class(0xFFFF0063),
    URLSearchParams => NOOP_CONSTRUCTOR_INFO; sealed crate::closure::shape::INTRINSIC_SERIAL_URL_SEARCH_PARAMS,
    URLPattern => crate::fn_info!(global_this_url_pattern_call_thunk, 2; with_declared(2)),
    AbortController => NOOP_CONSTRUCTOR_INFO,
    AbortSignal => NOOP_CONSTRUCTOR_INFO,
    EventTarget => NOOP_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::event_target::CLASS_ID_EVENT_TARGET),
    Crypto => WEBCRYPTO_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Crypto,
    CryptoKey => WEBCRYPTO_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::CryptoKey,
    SubtleCrypto => WEBCRYPTO_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::SubtleCrypto,
    Event => NOOP_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::event_target::CLASS_ID_EVENT),
    CustomEvent => NOOP_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::event_target::CLASS_ID_CUSTOM_EVENT),
    DOMException => NOOP_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::event_target::CLASS_ID_DOM_EXCEPTION),
    FormData => NOOP_CONSTRUCTOR_INFO,
    Blob => crate::fn_info!(global_this_blob_thunk, 2; with_declared(2)); instance BuiltinInstanceIdentity::Class(0xFFFF0026),
    File => crate::fn_info!(global_this_file_thunk, 3; with_declared(3)); instance BuiltinInstanceIdentity::Class(0xFFFF002F),
    Headers => crate::fn_info!(global_this_headers_thunk, 1; with_declared(1)); instance BuiltinInstanceIdentity::Class(0xFFFF002A),
    Request => crate::fn_info!(global_this_request_thunk, 2; with_declared(2)); instance BuiltinInstanceIdentity::Class(0xFFFF0029),
    Response => crate::fn_info!(global_this_response_thunk, 2; with_declared(2)); instance BuiltinInstanceIdentity::Class(0xFFFF0028),
    MessageChannel => crate::fn_info!(crate::messaging_hook_slots::js_message_channel_constructor_call_error, 0; with_declared(0)),
    MessagePort => crate::fn_info!(crate::messaging_hook_slots::js_message_port_constructor_call_error, 0; with_declared(0)),
    BroadcastChannel => crate::fn_info!(crate::messaging_hook_slots::js_broadcast_channel_constructor_call_error, 1; with_declared(1)),
    Storage => crate::fn_info!(crate::web_storage::storage_constructor_illegal, 0; with_declared(0)),
    WebSocket => NOOP_CONSTRUCTOR_INFO,
    FinalizationRegistry => NOOP_CONSTRUCTOR_INFO; class crate::weakref::CLASS_ID_FINALIZATION_REGISTRY; instance BuiltinInstanceIdentity::Class(crate::weakref::CLASS_ID_FINALIZATION_REGISTRY),
    // #2875: TC39 explicit-resource-management globals. Backed by the
    // no-op constructor thunk so `typeof DisposableStack === "function"`;
    // real `new DisposableStack()` / `new SuppressedError(...)` flow through
    // codegen's `lower_builtin_new` to the dedicated runtime ctors.
    DisposableStack => NOOP_CONSTRUCTOR_INFO,
    AsyncDisposableStack => NOOP_CONSTRUCTOR_INFO,
    SuppressedError => NOOP_CONSTRUCTOR_INFO,
    Buffer => NOOP_CONSTRUCTOR_INFO,
}

/// Is `name` one of the built-in CONSTRUCTORS installed on `globalThis`?
///
/// #7518: `try_dispatch_value_called_proto_method` needs this to tell a built-in
/// *prototype method* invoked as a value — the #3716 uncurry-this idiom, which it
/// must re-dispatch by name on the call's `this` — from a global *constructor*
/// invoked as a value, which it must not: `this.EventTarget(…)` resolves
/// to nothing and the by-name tower's catch-all throws
/// `TypeError: EventTarget is not a function`.
///
/// Both are backed by the same `global_this_builtin_noop_thunk`, and the only
/// thing that used to separate them was the accident that constructors carried no
/// recorded builtin `.length`. That stopped being true as
/// [`builtin_constructor_spec_length`] below grew to cover them, so the
/// distinction has to be stated rather than inferred.
pub(crate) fn is_global_this_builtin_constructor_name(name: &str) -> bool {
    GLOBAL_THIS_BUILTIN_CONSTRUCTORS
        .iter()
        .any(|decl| decl.name == name)
}

/// #3655: spec `length` (declared-parameter count) for each built-in
/// constructor, so `Ctor.length` reads the right arity through the runtime
/// value path (`const C = DataView; C.length === 1`) and
/// `Object.getOwnPropertyDescriptor(Ctor, 'length').value` matches Node. The
/// HIR also folds bare `Ctor.length` constants (`analysis::builtin_constructor_length`);
/// these are the runtime fallback for rebound / passed-as-value constructors.
/// Values verified against `node --experimental-strip-types`. Unlisted names
/// fall through to the closure arity registry (0).
pub(crate) fn builtin_constructor_spec_length(name: &str) -> Option<u32> {
    let len = match name {
        "Symbol"
        | "Map"
        | "Set"
        | "WeakMap"
        | "WeakSet"
        | "TextEncoder"
        | "TextDecoder"
        | "TextEncoderStream"
        | "TextDecoderStream"
        | "URLSearchParams"
        | "URLPattern"
        | "AbortController"
        | "AbortSignal"
        | "EventTarget"
        | "DOMException"
        | "FormData"
        | "Blob"
        | "Headers"
        | "Response"
        | "MessageChannel"
        | "MessagePort"
        | "Storage"
        | "Navigator"
        | "DisposableStack"
        | "AsyncDisposableStack" => 0,
        "CompressionStream" | "DecompressionStream" => 1,
        "Array"
        | "Object"
        | "String"
        | "Number"
        | "Boolean"
        | "Function"
        | "Error"
        | "TypeError"
        | "RangeError"
        | "SyntaxError"
        | "ReferenceError"
        | "EvalError"
        | "URIError"
        | "WeakRef"
        | "BigInt"
        | "ArrayBuffer"
        | "SharedArrayBuffer"
        | "DataView"
        | "URL"
        | "Event"
        | "CustomEvent"
        | "Request"
        | "WebSocket"
        | "BroadcastChannel"
        | "FinalizationRegistry"
        | "Promise" => 1,
        "RegExp" | "Proxy" | "AggregateError" | "File" => 2,
        "Date" => 7,
        "SuppressedError" | "Buffer" | "Uint8Array" | "Int8Array" | "Uint16Array"
        | "Int16Array" | "Uint32Array" | "Int32Array" | "Float16Array" | "Float32Array"
        | "Float64Array" | "Uint8ClampedArray" | "BigInt64Array" | "BigUint64Array" => 3,
        _ => return None,
    };
    Some(len)
}

/// JS built-in namespaces (typeof === "object", not "function"). Same
/// shape on the singleton — a backing object with `prototype` so chained
/// reads degrade gracefully — but typeof reports "object".
pub(crate) const GLOBAL_THIS_BUILTIN_NAMESPACES: &[&str] = &[
    "console",
    "process",
    "Math",
    "JSON",
    "Reflect",
    "Atomics",
    "Intl",
    "WebAssembly",
    // TC39 Temporal (#4686): typeof Temporal === "object". The constructors
    // (Temporal.Duration, …) and the Temporal.Now sub-namespace are hung off it
    // by `install_temporal_namespace`.
    "Temporal",
];

/// JS global built-in functions exposed as function-valued properties on
/// `globalThis`. Unlike constructor sentinels, these call through to Perry's
/// real direct-call runtime helpers so rebinding works:
/// `const clone = globalThis.structuredClone; clone(value)`.
pub(crate) const GLOBAL_THIS_BUILTIN_FUNCTIONS: &[&str] = &[
    "eval",
    "fetch",
    "structuredClone",
    "atob",
    "btoa",
    "setTimeout",
    "clearTimeout",
    "setInterval",
    "clearInterval",
    "setImmediate",
    "clearImmediate",
    "queueMicrotask",
    // The `gc()` builtin, exposed as a real callable global value so the
    // capability-guard idioms `if (globalThis.gc) gc()` / `global.gc?.()` /
    // `const f = gc; f()` work (Perry's `gc()` is always available, unlike
    // Node's `--expose-gc`-gated one).
    "gc",
    // #2905: standard global helper functions. These route through Perry's
    // real direct-call runtime helpers, so `const p = parseInt; p("42px")`
    // and `globalThis.encodeURIComponent("a b")` match Node.
    "parseInt",
    "parseFloat",
    "isNaN",
    "isFinite",
    "encodeURI",
    "decodeURI",
    "encodeURIComponent",
    "decodeURIComponent",
    // #4511: legacy escape/unescape (ES Annex B), used by `qs`.
    "escape",
    "unescape",
];

pub(crate) fn is_web_fetch_constructor(name: &str) -> bool {
    matches!(
        name,
        "Headers" | "Request" | "Response" | "Blob" | "File" | "FormData"
    )
}

/// The constructor's immutable header fact, after proving a function cell.
/// No body search, capture read, public property or global binding is consulted.
#[inline]
pub(crate) fn constructor_identity(value: f64) -> Option<BuiltinConstructorIdentity> {
    let jv = crate::JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let ptr = jv.as_pointer::<crate::closure::ClosureHeader>();
    if crate::value::addr_class::is_handle_band(ptr as usize)
        || !(ptr as usize).is_multiple_of(std::mem::align_of::<crate::closure::ClosureHeader>())
        || !super::is_valid_obj_ptr(ptr.cast())
    {
        return None;
    }
    unsafe {
        if !crate::closure::closure_kind_probe(ptr as usize) {
            return None;
        }
        let identity = BuiltinConstructorIdentity::from_header((*ptr).capture_count)?;
        debug_assert_eq!((*(*ptr).info).code, identity.declaration().info.code);
        Some(identity)
    }
}

/// Birth consumes one declaration, so code, header identity and call capture
/// cannot disagree. The fact precedes publication and survives moving GC.
pub(crate) fn alloc_builtin_constructor(
    identity: BuiltinConstructorIdentity,
) -> *mut crate::closure::ClosureHeader {
    let declaration = identity.declaration();
    let header = 1 | crate::closure::NO_THIS_REBIND_FLAG;
    let closure = crate::closure::js_closure_alloc(declaration.info, header);
    if !closure.is_null() {
        crate::closure::js_closure_set_capture_bits(
            closure,
            0,
            crate::JSValue::int32(identity as i32).bits(),
        );
        // Scalar declaration fact, installed before publication. The ordinary
        // birth has already checked the capture bound and initialized all slots.
        unsafe {
            (*closure).capture_count |=
                (identity as u32) << crate::codegen_abi::CLOSURE_CONSTRUCT_SHIFT;
        }
        debug_assert_eq!(unsafe { (*(*closure).info).code }, declaration.info.code);
    }
    closure
}

/// Call classification consumes the same birth fact as [[Construct]].
pub(crate) unsafe fn declaration_of_constructor(
    closure: *const crate::closure::ClosureHeader,
    info: &crate::closure::JsFunctionInfo,
) -> Option<&'static BuiltinConstructorDeclaration> {
    let identity = BuiltinConstructorIdentity::from_header((*closure).capture_count)?;
    let declaration = identity.declaration();
    debug_assert_eq!(declaration.info.code, info.code);
    Some(declaration)
}
