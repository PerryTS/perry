//! Static `globalThis` constructor/function metadata.

use super::global_this::*;

const NOOP_CONSTRUCTOR_INFO: *const crate::closure::JsFunctionInfo =
    crate::fn_info!(#[realm(realm_decl! { WebSocket.prototype.send; CompressionStream, kind = constructor, provider = builtin_constructor, births = [ArrayBuffer, TransformStream, Uint8Array]; AbortController, kind = constructor, provider = builtin_constructor, births = [AbortSignal, Event], spec = {length = 0}, effects = { stores(fresh); returns(fresh(AbortController.prototype, slots = {signal: fresh(AbortSignal.prototype)})) }, emit = metadata(realm_urlpattern_metadata); AsyncDisposableStack, kind = constructor, provider = builtin_constructor, births = [Promise, SuppressedError], spec = {length = 0}, emit = metadata(realm_urlpattern_metadata); DisposableStack, kind = constructor, provider = builtin_constructor, births = [SuppressedError], spec = {length = 0}, emit = metadata(realm_urlpattern_metadata); FormData, kind = constructor, provider = builtin_constructor, births = ["%IteratorPrototype%", Array, Blob, File], spec = {length = 0}, emit = metadata(realm_urlpattern_metadata); Navigator, kind = constructor, provider = builtin_constructor, spec = {length = 0}, emit = metadata(realm_urlpattern_metadata); TextDecoder, kind = constructor, provider = builtin_constructor, births = [Uint8Array], spec = {length = 0}, emit = metadata(realm_urlpattern_metadata); TextDecoderStream, kind = constructor, provider = builtin_constructor, births = [TextDecoder, TransformStream, Uint8Array], spec = {length = 0}, emit = metadata(realm_urlpattern_metadata); TextEncoder, kind = constructor, provider = builtin_constructor, births = [Uint8Array], spec = {length = 0}, emit = metadata(realm_urlpattern_metadata); TextEncoderStream, kind = constructor, provider = builtin_constructor, births = [TextEncoder, TransformStream, Uint8Array], spec = {length = 0}, emit = metadata(realm_urlpattern_metadata); URLSearchParams, kind = constructor, provider = builtin_constructor, births = ["%IteratorPrototype%", Array], spec = {length = 0}, emit = metadata(realm_urlpattern_metadata); Event, kind = constructor, provider = builtin_constructor, members = { NONE = 0.0, attrs = enumerable; CAPTURING_PHASE = 1.0, attrs = enumerable; AT_TARGET = 2.0, attrs = enumerable; BUBBLING_PHASE = 3.0, attrs = enumerable; }, effects = { coerce(arg(0), ToPrimitive); reads(field(arg(1), bubbles), field(arg(1), cancelable), field(arg(1), composed)); returns(fresh(Event.prototype)) }; WebSocket, kind = constructor, provider = builtin_constructor, members = { CONNECTING = 0.0, attrs = enumerable; OPEN = 1.0, attrs = enumerable; CLOSING = 2.0, attrs = enumerable; CLOSED = 3.0, attrs = enumerable; }; URL, kind = constructor, provider = builtin_constructor, members = { canParse; parse; }, births = [URLSearchParams]; CustomEvent, kind = constructor, provider = builtin_constructor, extends = Event, effects = { coerce(arg(0), ToPrimitive); reads(field(arg(1), bubbles), field(arg(1), cancelable), field(arg(1), composed)); stores(field(arg(1), detail) -> field(result, detail)); returns(fresh(CustomEvent.prototype)) }; MessageEvent, kind = constructor, provider = builtin_constructor, extends = Event, births = [Array, MessagePort]; SuppressedError, kind = constructor, provider = builtin_constructor, extends = Error, spec = {length = 3}, emit = metadata(realm_suppressederror_metadata); AbortSignal, kind = constructor, provider = builtin_constructor, extends = EventTarget, members = { abort; timeout; any; }, spec = {length = 0}, emit = metadata(realm_urlpattern_metadata), births = [Array, DOMException, Event, Promise]; AggregateError, kind = constructor, provider = builtin_constructor, extends = Error, spec = {length = 2}, emit = metadata(realm_file_metadata), births = [Array]; DOMException, kind = constructor, provider = builtin_constructor, members = { INDEX_SIZE_ERR = 1.0, attrs = enumerable; DOMSTRING_SIZE_ERR = 2.0, attrs = enumerable; HIERARCHY_REQUEST_ERR = 3.0, attrs = enumerable; WRONG_DOCUMENT_ERR = 4.0, attrs = enumerable; INVALID_CHARACTER_ERR = 5.0, attrs = enumerable; NO_DATA_ALLOWED_ERR = 6.0, attrs = enumerable; NO_MODIFICATION_ALLOWED_ERR = 7.0, attrs = enumerable; NOT_FOUND_ERR = 8.0, attrs = enumerable; NOT_SUPPORTED_ERR = 9.0, attrs = enumerable; INUSE_ATTRIBUTE_ERR = 10.0, attrs = enumerable; INVALID_STATE_ERR = 11.0, attrs = enumerable; SYNTAX_ERR = 12.0, attrs = enumerable; INVALID_MODIFICATION_ERR = 13.0, attrs = enumerable; NAMESPACE_ERR = 14.0, attrs = enumerable; INVALID_ACCESS_ERR = 15.0, attrs = enumerable; VALIDATION_ERR = 16.0, attrs = enumerable; TYPE_MISMATCH_ERR = 17.0, attrs = enumerable; SECURITY_ERR = 18.0, attrs = enumerable; NETWORK_ERR = 19.0, attrs = enumerable; ABORT_ERR = 20.0, attrs = enumerable; URL_MISMATCH_ERR = 21.0, attrs = enumerable; QUOTA_EXCEEDED_ERR = 22.0, attrs = enumerable; TIMEOUT_ERR = 23.0, attrs = enumerable; INVALID_NODE_TYPE_ERR = 24.0, attrs = enumerable; DATA_CLONE_ERR = 25.0, attrs = enumerable; }, spec = {length = 0}, emit = metadata(realm_urlpattern_metadata); EventTarget, kind = constructor, provider = builtin_constructor, members = { ["@for:nodejs.event_target"] = true, attrs = writable | enumerable | configurable; }, spec = {length = 0}, emit = metadata(realm_urlpattern_metadata), births = [Event]; Proxy, kind = constructor, provider = builtin_constructor, members = { revocable; }, spec = {length = 2}, emit = metadata(realm_file_metadata); DecompressionStream, kind = constructor, provider = builtin_constructor, births = [ArrayBuffer, TransformStream, Uint8Array]; FinalizationRegistry, kind = constructor, provider = builtin_constructor; ReadableStream, kind = constructor, provider = builtin_constructor, members = { from, attrs = writable | enumerable | configurable; }, spec = {length = 0}, emit = metadata(realm_stream_constructor_public), births = ["%AsyncIteratorPrototype%", ArrayBuffer, Promise, Uint8Array], effects = { reads(field(arg(1), size), field(arg(1), highWaterMark), field(arg(0), type), field(arg(0), start), field(arg(0), pull), field(arg(0), cancel), field(arg(0), autoAllocateChunkSize)); coerce(field(arg(1), highWaterMark), ToPrimitive); coerce(field(arg(0), type), ToPrimitive); calls(field(arg(0), start), this = arg(0), args = [fresh(ReadableStreamDefaultController.prototype, slots = {stream: result}) | fresh(ReadableByteStreamController.prototype, slots = {stream: result, byobRequest: fresh(ReadableStreamBYOBRequest.prototype, slots = {stream: result})})]); calls(field(arg(0), pull), this = arg(0), args = [fresh(ReadableStreamDefaultController.prototype, slots = {stream: result}) | fresh(ReadableByteStreamController.prototype, slots = {stream: result, byobRequest: fresh(ReadableStreamBYOBRequest.prototype, slots = {stream: result})})]); calls(field(arg(0), cancel), this = arg(0), args = [islot(result, cancelReason)]); calls(field(arg(1), size), this = prim(undefined), args = [islot(result, queue)]); coerce(returned(3), ToPrimitive); stores(fresh); returns(fresh(ReadableStream.prototype, slots = {started: promise(returned(0)), pulled: promise(returned(1)), cancelled: promise(returned(2)), queue: fresh(Uint8Array.prototype)})) }; ReadableStreamDefaultController, kind = constructor, provider = builtin_constructor, spec = {length = 0}, emit = metadata(realm_urlpattern_metadata); ReadableStreamDefaultReader, kind = constructor, provider = builtin_constructor, births = [Promise]; ReadableByteStreamController, kind = constructor, provider = builtin_constructor, spec = {length = 0}, emit = metadata(realm_urlpattern_metadata); ReadableStreamBYOBReader, kind = constructor, provider = builtin_constructor, births = [Promise]; ReadableStreamBYOBRequest, kind = constructor, provider = builtin_constructor, spec = {length = 0}, emit = metadata(realm_urlpattern_metadata); TransformStream, kind = constructor, provider = builtin_constructor, spec = {length = 0}, emit = metadata(realm_stream_constructor_public), births = [ReadableStream, WritableStream]; WritableStream, kind = constructor, provider = builtin_constructor, spec = {length = 0}, emit = metadata(realm_stream_constructor_public), births = [ArrayBuffer, Promise, Uint8Array]; })] export crate::object::global_this::fetch_globals::global_this_builtin_noop_thunk, 1);
/// `%TypedArray%`: the abstract parent family of the 12 typed-array families
/// (declared `extends = "%TypedArray%"`); no globalThis binding.
const TYPED_ARRAY_INTRINSIC_INFO: *const crate::closure::JsFunctionInfo =
    crate::fn_info!(#[realm(realm_decl! { BigInt64Array, kind = constructor, provider = builtin_constructor, extends = "%TypedArray%", members = { BYTES_PER_ELEMENT = 8.0; }, spec = {length = 3}, emit = metadata(realm_suppressederror_metadata); BigUint64Array, kind = constructor, provider = builtin_constructor, extends = "%TypedArray%", members = { BYTES_PER_ELEMENT = 8.0; }, spec = {length = 3}, emit = metadata(realm_suppressederror_metadata); Float16Array, kind = constructor, provider = builtin_constructor, extends = "%TypedArray%", members = { BYTES_PER_ELEMENT = 2.0; }, spec = {length = 3}, emit = metadata(realm_suppressederror_metadata); Float32Array, kind = constructor, provider = builtin_constructor, extends = "%TypedArray%", members = { BYTES_PER_ELEMENT = 4.0; }, spec = {length = 3}, emit = metadata(realm_suppressederror_metadata); Float64Array, kind = constructor, provider = builtin_constructor, extends = "%TypedArray%", members = { BYTES_PER_ELEMENT = 8.0; }, spec = {length = 3}, emit = metadata(realm_suppressederror_metadata); Int16Array, kind = constructor, provider = builtin_constructor, extends = "%TypedArray%", members = { BYTES_PER_ELEMENT = 2.0; }, spec = {length = 3}, emit = metadata(realm_suppressederror_metadata); Int32Array, kind = constructor, provider = builtin_constructor, extends = "%TypedArray%", members = { BYTES_PER_ELEMENT = 4.0; }, spec = {length = 3}, emit = metadata(realm_suppressederror_metadata); Int8Array, kind = constructor, provider = builtin_constructor, extends = "%TypedArray%", members = { BYTES_PER_ELEMENT = 1.0; }, spec = {length = 3}, emit = metadata(realm_suppressederror_metadata); Uint16Array, kind = constructor, provider = builtin_constructor, extends = "%TypedArray%", members = { BYTES_PER_ELEMENT = 2.0; }, spec = {length = 3}, emit = metadata(realm_suppressederror_metadata); Uint32Array, kind = constructor, provider = builtin_constructor, extends = "%TypedArray%", members = { BYTES_PER_ELEMENT = 4.0; }, spec = {length = 3}, emit = metadata(realm_suppressederror_metadata); Uint8Array, kind = constructor, provider = builtin_constructor, extends = "%TypedArray%", members = { fromBase64; fromHex; BYTES_PER_ELEMENT = 1.0; }, spec = {length = 3}, emit = metadata(realm_suppressederror_metadata); Uint8ClampedArray, kind = constructor, provider = builtin_constructor, extends = "%TypedArray%", members = { BYTES_PER_ELEMENT = 1.0; }, spec = {length = 3}, emit = metadata(realm_suppressederror_metadata); "%TypedArray%", kind = constructor, provider = builtin_constructor, members = { of; from; [Symbol.species] = accessor(get = "%TypedArray%[\"@@species\"].<get>"); }, spec = {name = "TypedArray"}, emit = metadata(realm_typed_array_public), births = [ArrayBuffer]; })] export crate::object::global_this::ctor_thunks::typed_array_constructor_call_thunk, 1; with_declared(0));
const TYPED_ARRAY_CONSTRUCTOR_INFO: *const crate::closure::JsFunctionInfo =
    crate::fn_info!(typed_array_constructor_call_thunk, 1; with_declared(0));
const CONSTRUCT_ONLY_INFO: *const crate::closure::JsFunctionInfo =
    crate::fn_info!(#[realm(realm_decl! { DataView, kind = constructor, provider = builtin_constructor, births = [ArrayBuffer], spec = {length = 1}, emit = metadata(realm_function_metadata); ArrayBuffer, kind = constructor, provider = builtin_constructor, members = { isView; [Symbol.species] = accessor(get = "ArrayBuffer[\"@@species\"].<get>"); }, spec = {length = 1}, emit = metadata(realm_function_metadata); SharedArrayBuffer, kind = constructor, provider = builtin_constructor, members = { [Symbol.species] = accessor(get = "SharedArrayBuffer[\"@@species\"].<get>"); }, spec = {length = 1}, emit = metadata(realm_function_metadata); Iterator, kind = constructor, provider = builtin_constructor, instance = "%IteratorPrototype%", members = { from; concat; }; })] export crate::object::global_this::ctor_thunks::construct_only_builtin_call_thunk, 0);
const WEBCRYPTO_CONSTRUCTOR_INFO: *const crate::closure::JsFunctionInfo =
    crate::fn_info!(#[realm(realm_decl! { Crypto, kind = constructor, provider = builtin_constructor, births = [ArrayBuffer, CryptoKey, DOMException, Promise, SubtleCrypto, Uint8Array]; CryptoKey, kind = constructor, provider = builtin_constructor; SubtleCrypto, kind = constructor, provider = builtin_constructor, members = { supports, attrs = writable | enumerable | configurable; }, births = [ArrayBuffer, CryptoKey, DOMException, Promise, Uint8Array]; })] export crate::object::global_this::ctor_thunks::webcrypto_illegal_constructor_thunk, 0);

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
    /// The family is born on first observation, not at realm setup: setup
    /// allocates only the constructor (the realm's [[Intrinsics]] entry holds
    /// it, and `globalThis.<name>` binds it); `realm_intrinsic_addr`, the one
    /// materialize point, births the rest (`global_this::populate::birth_family`)
    /// the first time the intrinsic prototype is asked for. The constructor
    /// has no own property bag until then: its `length`, `name` and
    /// `prototype` are the ordinary not-yet-materialized intrinsic slots, and
    /// whatever needs the bag or the prototype births the family first.
    pub lazy: bool,
    /// The family's members are provided outside the runtime: its birth
    /// calls the registered provider hook (`external_family.rs`). Always `lazy`.
    pub external: bool,
    /// A `lazy` constructor's own static keys, in creation order after
    /// `length`, `name` and `prototype` (its declared static slots): while
    /// the family is unborn, presence and key listing answer them from here;
    /// reading one's value births the family, which installs it. The realm
    /// source's own-key order is the check (`lazy_family_tests`).
    pub statics: &'static [&'static str],
    /// `class C extends P`: the family whose prototype is the
    /// [[Prototype]] of `C.prototype`, linked by `C`'s birth.
    pub extends: Option<BuiltinConstructorIdentity>,
    /// The family whose constructor is the [[Prototype]] of `C`
    /// (`extends` minus the interfaces whose constructor does not inherit).
    pub constructor_extends: Option<BuiltinConstructorIdentity>,
    /// `get C[Symbol.species]` returning `this` (#11193), defined by its birth.
    pub species: bool,
    /// The realm's name of the intrinsic (`Map`, `%TypedArray%`).
    #[cfg(test)]
    pub realm_name: &'static str,
    /// %Global% binds the constructor under `name` (the row's `global`).
    pub global: bool,
    /// A family that cannot be directly constructed throws this declared TypeError.
    pub construct_error: Option<&'static [u8]>,
}

// `declared_family_links!(Name)` = (extends, constructor_extends, species), generated by
// build.rs from the realm declaration (perry-runtime-manifest realm_global_bindings.rs).
include!(concat!(env!("OUT_DIR"), "/family_links.rs"));

macro_rules! builtin_constructors {
    ($($name:ident => $info:expr $(; class $class:expr)? $(; serial $serial:expr)? $(; sealed $sealed:expr)? $(; instance $instance:expr)? $(; prototype_callable $callable:expr)? $(; named_serial $named:expr)? $(; lazy $lazy:expr)? $(; external $external:expr)? $(; global $global:tt)? $(; construct_error $construct_error:literal)? $(; statics [$($static:literal),* $(,)?])?),* $(,)?) => {
        /// Immutable [[Construct]] identity in bits 23..29 of the function header.
        /// Zero belongs to every ordinary, wrapper and bound function.
        #[repr(u8)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[allow(dead_code)] // `None` is the zero encoding
        pub enum BuiltinConstructorIdentity { None = 0, $($name,)* }
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
            /// The declaration whose intrinsic is named `name` (the spec name
            /// of the realm intrinsic, never the current `globalThis` binding).
            pub(crate) fn for_name(name: &str) -> Option<Self> {
                match name { $(n if n == builtin_constructors!(@realm_name $name $($global)?) => Some(Self::$name),)* _ => None }
            }
        }
        const _: () = assert!((BuiltinConstructorIdentity::Buffer as u32) < 128);
        /// Builtin declarations, in the order captured by shared constructor bodies.
        pub(crate) const GLOBAL_THIS_BUILTIN_CONSTRUCTORS: &[BuiltinConstructorDeclaration] = &[
            $(BuiltinConstructorDeclaration {
                identity: BuiltinConstructorIdentity::$name,
                name: stringify!($name),
                #[cfg(test)]
                realm_name: builtin_constructors!(@realm_name $name $($global)?),
                global: builtin_constructors!(@global $($global)?),
                construct_error: builtin_constructors!(@optional $($construct_error)?),
                // SAFETY: fn_info! returns its own static immutable record.
                info: unsafe { &*$info },
                prototype_class: builtin_constructors!(@optional $($class)?),
                prototype_serial: builtin_constructors!(@optional $($serial)?),
                prototype_callable: builtin_constructors!(@callable $($callable)?),
                sealed_serial: builtin_constructors!(@optional $($sealed)?),
                named_serial: builtin_constructors!(@optional $($named)?),
                instance_identity: builtin_constructors!(@optional $($instance)?),
                lazy: builtin_constructors!(@lazy $($lazy)?),
                external: builtin_constructors!(@callable $($external)?),
                statics: &[$($($static,)*)?],
                // Derived by build.rs from the realm declaration (family_links.rs).
                extends: declared_family_links!($name).0,
                constructor_extends: declared_family_links!($name).1,
                species: declared_family_links!($name).2,
            },)*
        ];
    };
    // Every family is born on its first observation, except the root
    // family Object (every prototype chain observes it): born at setup.
    // A row's globalThis binding is a property of the row: `global false`
    // declares a family the realm holds but %Global% does not bind; its realm
    // name is the spec's `%Name%` (`%TypedArray%`).
    (@global $global:tt) => { $global };
    (@global) => { true };
    (@realm_name $name:ident false) => { concat!("%", stringify!($name), "%") };
    (@realm_name $name:ident $($global:tt)?) => { stringify!($name) };
    (@lazy $lazy:expr) => { $lazy };
    (@lazy) => { true };
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
    Array => ARRAY_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(0xFFFF0024); named_serial crate::closure::shape::INTRINSIC_SERIAL_ARRAY_PROTOTYPE; statics ["isArray", "from", "of"],
    Object => crate::fn_info!(#[realm(Object, kind = constructor, provider = builtin_constructor, members = { keys; values; entries; freeze; create; seal; isSealed; isFrozen; isExtensible; preventExtensions; is; setPrototypeOf; getOwnPropertySymbols; getOwnPropertyDescriptors; defineProperties; groupBy; getPrototypeOf; getOwnPropertyNames; getOwnPropertyDescriptor; defineProperty; fromEntries; assign; hasOwn; })] export crate::object::global_this::builtin_thunks::global_this_object_thunk, 1; with_declared(1)); instance BuiltinInstanceIdentity::Class(0xFFFF0050); lazy false; statics ["keys", "values", "entries", "freeze", "create", "seal", "isSealed", "isFrozen", "isExtensible", "preventExtensions", "is", "setPrototypeOf", "getOwnPropertySymbols", "getOwnPropertyDescriptors", "defineProperties", "groupBy", "getPrototypeOf", "getOwnPropertyNames", "getOwnPropertyDescriptor", "defineProperty", "fromEntries", "assign", "hasOwn"],
    String => crate::fn_info!(#[realm(String, kind = constructor, provider = builtin_constructor, members = { fromCharCode; fromCodePoint; raw; }, births = ["%StringIteratorPrototype%", Array, RegExp])] export crate::object::global_this::builtin_thunks::global_this_string_thunk, 1; with_declared(1)); instance BuiltinInstanceIdentity::Class(0xFFFF00D1); statics ["fromCharCode", "fromCodePoint", "raw"],
    Number => crate::fn_info!(#[realm(Number, kind = constructor, provider = builtin_constructor, members = { isNaN; isFinite; isInteger; isSafeInteger; parseFloat = ref(parseFloat); parseInt = ref(parseInt); NaN = NaN; POSITIVE_INFINITY = Infinity; NEGATIVE_INFINITY = -Infinity; MAX_VALUE = 1.7976931348623157e+308; MIN_VALUE = 5e-324; EPSILON = 2.220446049250313e-16; MAX_SAFE_INTEGER = 9007199254740991.0; MIN_SAFE_INTEGER = -9007199254740991.0; })] export crate::object::global_this::builtin_thunks::global_this_number_thunk, 1; with_declared(1)); instance BuiltinInstanceIdentity::Class(0xFFFF00D0); statics ["isNaN", "isFinite", "isInteger", "isSafeInteger", "parseFloat", "parseInt", "NaN", "POSITIVE_INFINITY", "NEGATIVE_INFINITY", "MAX_VALUE", "MIN_VALUE", "EPSILON", "MAX_SAFE_INTEGER", "MIN_SAFE_INTEGER"],
    Boolean => crate::fn_info!(#[realm(Boolean, kind = constructor, provider = builtin_constructor)] export crate::object::global_this::builtin_thunks::global_this_boolean_thunk, 1; with_declared(1)); instance BuiltinInstanceIdentity::Class(0xFFFF00D2),
    Function => crate::fn_info!(#[realm(Function, kind = constructor, provider = builtin_constructor, spec = {length = 1}, emit = metadata(realm_function_metadata))] export unwind_in_tests crate::object::global_this::builtin_thunks::global_this_function_call_thunk, 1; with_rest(0)); serial crate::closure::shape::INTRINSIC_SERIAL_FUNCTION; instance BuiltinInstanceIdentity::Class(0xFFFF00F0); prototype_callable true; lazy false,
    RegExp => crate::fn_info!(#[realm(RegExp, kind = constructor, provider = builtin_constructor, members = { escape; [Symbol.species] = accessor(get = "RegExp[\"@@species\"].<get>"); }, births = [Array, String])] export crate::object::global_this::ctor_thunks::regexp_constructor_call_thunk, 2; with_declared(2)); instance BuiltinInstanceIdentity::Class(0xFFFF0021); statics ["escape"],
    Date => crate::fn_info!(#[realm(Date, kind = constructor, provider = builtin_constructor, members = { now; parse; UTC; }, spec = {length = 7}, emit = metadata(realm_date_metadata))] export crate::object::global_this::fetch_globals::global_this_date_thunk, 1; with_declared(1)); instance BuiltinInstanceIdentity::Class(0xFFFF0020); statics ["now", "parse", "UTC"],
    Error => crate::fn_info!(#[realm(Error, kind = constructor, provider = builtin_constructor, members = { captureStackTrace; isError; prepareStackTrace; stackTraceLimit = 10.0, attrs = writable | enumerable | configurable; })] export crate::object::global_this::ctor_thunks::error_constructor_call_thunk, 1; with_declared(1)); instance BuiltinInstanceIdentity::Class(crate::error::CLASS_ID_ERROR); statics ["captureStackTrace", "isError", "prepareStackTrace", "stackTraceLimit"],
    TypeError => crate::fn_info!(#[realm(TypeError, kind = constructor, provider = builtin_constructor, extends = Error)] export crate::object::global_this::ctor_thunks::type_error_constructor_call_thunk, 1; with_declared(1)); instance BuiltinInstanceIdentity::Class(crate::error::CLASS_ID_TYPE_ERROR),
    RangeError => crate::fn_info!(#[realm(RangeError, kind = constructor, provider = builtin_constructor, extends = Error)] export crate::object::global_this::ctor_thunks::range_error_constructor_call_thunk, 1; with_declared(1)); instance BuiltinInstanceIdentity::Class(crate::error::CLASS_ID_RANGE_ERROR),
    SyntaxError => crate::fn_info!(#[realm(SyntaxError, kind = constructor, provider = builtin_constructor, extends = Error, attrs = {prototype = writable})] export crate::object::global_this::ctor_thunks::syntax_error_constructor_call_thunk, 1; with_declared(1)); instance BuiltinInstanceIdentity::Class(crate::error::CLASS_ID_SYNTAX_ERROR),
    ReferenceError => crate::fn_info!(#[realm(ReferenceError, kind = constructor, provider = builtin_constructor, extends = Error)] export crate::object::global_this::ctor_thunks::reference_error_constructor_call_thunk, 1; with_declared(1)); instance BuiltinInstanceIdentity::Class(crate::error::CLASS_ID_REFERENCE_ERROR),
    EvalError => crate::fn_info!(#[realm(EvalError, kind = constructor, provider = builtin_constructor, extends = Error)] export crate::object::global_this::ctor_thunks::eval_error_constructor_call_thunk, 1; with_declared(1)); instance BuiltinInstanceIdentity::Class(crate::error::CLASS_ID_EVAL_ERROR),
    URIError => crate::fn_info!(#[realm(URIError, kind = constructor, provider = builtin_constructor, extends = Error)] export crate::object::global_this::ctor_thunks::uri_error_constructor_call_thunk, 1; with_declared(1)); instance BuiltinInstanceIdentity::Class(crate::error::CLASS_ID_URI_ERROR),
    AggregateError => NOOP_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::error::CLASS_ID_AGGREGATE_ERROR),
    Symbol => crate::fn_info!(#[realm(Symbol, kind = constructor, provider = builtin_constructor, members = { for; keyFor; iterator = Symbol.iterator; asyncIterator = Symbol.asyncIterator; }, spec = {length = 0}, emit = metadata(realm_urlpattern_metadata))] export crate::object::global_this::builtin_thunks::global_this_symbol_thunk, 1; with_declared(1)); instance BuiltinInstanceIdentity::Class(0xFFFF00D4); statics ["for", "keyFor", "iterator", "asyncIterator"],
    Promise => crate::fn_info!(#[realm(Promise, kind = constructor, provider = builtin_constructor, members = { resolve; reject; all; race; allSettled; any; withResolvers; try; [Symbol.species] = accessor(get = "Promise[\"@@species\"].<get>"); }, births = [Array])] export crate::object::global_this::ctor_thunks::promise_constructor_call_thunk, 1); instance BuiltinInstanceIdentity::Class(0xFFFF0027); statics ["resolve", "reject", "all", "race", "allSettled", "any", "withResolvers", "try"],
    Map => crate::fn_info!(#[realm(Map, kind = constructor, provider = builtin_constructor, members = { groupBy; [Symbol.species] = accessor(get = "Map[\"@@species\"].<get>"); }, spec = {length = 0}, emit = metadata(realm_urlpattern_metadata), births = ["%MapIteratorPrototype%", Array])] export crate::object::global_this::ctor_thunks::map_constructor_call_thunk, 1); instance BuiltinInstanceIdentity::Class(0xFFFF0022); statics ["groupBy"],
    Set => crate::fn_info!(#[realm(Set, kind = constructor, provider = builtin_constructor, members = { [Symbol.species] = accessor(get = "Set[\"@@species\"].<get>"); }, spec = {length = 0}, emit = metadata(realm_urlpattern_metadata), births = ["%SetIteratorPrototype%", Array])] export crate::object::global_this::ctor_thunks::set_constructor_call_thunk, 1); instance BuiltinInstanceIdentity::Class(0xFFFF0023),
    WeakMap => crate::fn_info!(#[realm(WeakMap, kind = constructor, provider = builtin_constructor, spec = {length = 0}, emit = metadata(realm_urlpattern_metadata))] export crate::object::global_this::ctor_thunks::weak_map_constructor_call_thunk, 1); class crate::weakref::CLASS_ID_WEAKMAP; instance BuiltinInstanceIdentity::Class(0xFFFF002C),
    WeakSet => crate::fn_info!(#[realm(WeakSet, kind = constructor, provider = builtin_constructor, spec = {length = 0}, emit = metadata(realm_urlpattern_metadata))] export crate::object::global_this::ctor_thunks::weak_set_constructor_call_thunk, 1); class crate::weakref::CLASS_ID_WEAKSET; instance BuiltinInstanceIdentity::Class(0xFFFF002D),
    WeakRef => crate::fn_info!(#[realm(WeakRef, kind = constructor, provider = builtin_constructor)] export crate::object::global_this::ctor_thunks::weak_ref_constructor_call_thunk, 1); class crate::weakref::CLASS_ID_WEAKREF; instance BuiltinInstanceIdentity::Class(crate::weakref::CLASS_ID_WEAKREF),
    Proxy => NOOP_CONSTRUCTOR_INFO; statics ["revocable"],
    BigInt => crate::fn_info!(#[realm(BigInt, kind = constructor, provider = builtin_constructor, members = { asIntN; asUintN; })] export crate::object::global_this::builtin_thunks::global_this_bigint_thunk, 1; with_declared(1)); instance BuiltinInstanceIdentity::Class(0xFFFF00D3); statics ["asIntN", "asUintN"],
    TypedArray => TYPED_ARRAY_INTRINSIC_INFO; global false; construct_error b"Abstract class TypedArray not directly constructable"; statics ["of", "from"],
    Uint8Array => TYPED_ARRAY_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::typedarray_hook_slots::CLASS_ID_UINT8_ARRAY); statics ["fromBase64", "fromHex", "BYTES_PER_ELEMENT"],
    Int8Array => TYPED_ARRAY_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::typedarray_hook_slots::CLASS_ID_INT8_ARRAY); statics ["BYTES_PER_ELEMENT"],
    Uint16Array => TYPED_ARRAY_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::typedarray_hook_slots::CLASS_ID_UINT16_ARRAY); statics ["BYTES_PER_ELEMENT"],
    Int16Array => TYPED_ARRAY_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::typedarray_hook_slots::CLASS_ID_INT16_ARRAY); statics ["BYTES_PER_ELEMENT"],
    Uint32Array => TYPED_ARRAY_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::typedarray_hook_slots::CLASS_ID_UINT32_ARRAY); statics ["BYTES_PER_ELEMENT"],
    Int32Array => TYPED_ARRAY_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::typedarray_hook_slots::CLASS_ID_INT32_ARRAY); statics ["BYTES_PER_ELEMENT"],
    Float16Array => TYPED_ARRAY_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::typedarray_hook_slots::CLASS_ID_FLOAT16_ARRAY); statics ["BYTES_PER_ELEMENT"],
    Float32Array => TYPED_ARRAY_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::typedarray_hook_slots::CLASS_ID_FLOAT32_ARRAY); statics ["BYTES_PER_ELEMENT"],
    Float64Array => TYPED_ARRAY_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::typedarray_hook_slots::CLASS_ID_FLOAT64_ARRAY); statics ["BYTES_PER_ELEMENT"],
    Uint8ClampedArray => TYPED_ARRAY_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::typedarray_hook_slots::CLASS_ID_UINT8_CLAMPED_ARRAY); statics ["BYTES_PER_ELEMENT"],
    BigInt64Array => TYPED_ARRAY_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::typedarray_hook_slots::CLASS_ID_BIGINT64_ARRAY); statics ["BYTES_PER_ELEMENT"],
    BigUint64Array => TYPED_ARRAY_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::typedarray_hook_slots::CLASS_ID_BIGUINT64_ARRAY); statics ["BYTES_PER_ELEMENT"],
    ArrayBuffer => CONSTRUCT_ONLY_INFO; instance BuiltinInstanceIdentity::Class(0xFFFF0025); statics ["isView"],
    SharedArrayBuffer => CONSTRUCT_ONLY_INFO; instance BuiltinInstanceIdentity::Class(0xFFFF002E),
    DataView => CONSTRUCT_ONLY_INFO; sealed crate::closure::shape::INTRINSIC_SERIAL_DATA_VIEW_PROTOTYPE; instance BuiltinInstanceIdentity::Class(0xFFFF002B),
    TextEncoder => NOOP_CONSTRUCTOR_INFO; lazy true,
    TextDecoder => NOOP_CONSTRUCTOR_INFO; lazy true,
    TextEncoderStream => NOOP_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::object::CLASS_ID_TEXT_ENCODER_STREAM); lazy true,
    TextDecoderStream => NOOP_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::object::CLASS_ID_TEXT_DECODER_STREAM); lazy true,
    CompressionStream => NOOP_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::object::CLASS_ID_COMPRESSION_STREAM); lazy true,
    DecompressionStream => NOOP_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::object::CLASS_ID_DECOMPRESSION_STREAM); lazy true,
    // The three core Web Streams constructors. Perry implements them (codegen
    // lowers `new ReadableStream(…)` and `x instanceof ReadableStream`, and the
    // class has an id), but the NAMES were never registered here or in codegen's
    // globalThis-builtin list — so a bare `ReadableStream` identifier resolved to
    // nothing: `typeof ReadableStream === "undefined"` and `"ReadableStream" in
    // globalThis` was false, whereas Node exposes all three as functions.
    // Libraries feature-detect precisely that (`typeof ReadableStream !==
    // "undefined" ? … : …`) when deciding how to consume a `fetch()` body.
    // RSTREAM: the three ReadableStream families are born on first
    // observation; their members come from the program's streams install
    // (an `external` family: `external_family.rs`).
    ReadableStream => NOOP_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(0xFFFF0060); lazy true; external true; statics ["from"],
    ReadableStreamDefaultController => NOOP_CONSTRUCTOR_INFO; lazy true; external true,
    ReadableStreamDefaultReader => NOOP_CONSTRUCTOR_INFO; lazy true; external true,
    // The byte-stream families (`type: "bytes"`): same external lazy contract.
    ReadableByteStreamController => NOOP_CONSTRUCTOR_INFO; lazy true; external true,
    ReadableStreamBYOBReader => NOOP_CONSTRUCTOR_INFO; lazy true; external true,
    ReadableStreamBYOBRequest => NOOP_CONSTRUCTOR_INFO; lazy true; external true,
    WritableStream => NOOP_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(0xFFFF0061); lazy true,
    TransformStream => NOOP_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(0xFFFF0062); lazy true,
    Navigator => NOOP_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::navigator::NAVIGATOR_CLASS_ID); lazy true,
    URL => NOOP_CONSTRUCTOR_INFO; sealed crate::closure::shape::INTRINSIC_SERIAL_URL; instance BuiltinInstanceIdentity::Class(0xFFFF0063); statics ["canParse", "parse"],
    URLSearchParams => NOOP_CONSTRUCTOR_INFO; sealed crate::closure::shape::INTRINSIC_SERIAL_URL_SEARCH_PARAMS,
    URLPattern => crate::fn_info!(#[realm(URLPattern, kind = constructor, provider = builtin_constructor, spec = {length = 0}, emit = metadata(realm_urlpattern_metadata))] export crate::object::global_this::ctor_thunks::global_this_url_pattern_call_thunk, 2; with_declared(2)); lazy true,
    // A parent family is declared before its `extends` children: a
    // child's birth observes (births) its parent.
    EventTarget => NOOP_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::event_target::CLASS_ID_EVENT_TARGET),
    AbortController => NOOP_CONSTRUCTOR_INFO,
    AbortSignal => NOOP_CONSTRUCTOR_INFO; statics ["abort", "timeout", "any"],
    Crypto => WEBCRYPTO_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Crypto; lazy true,
    CryptoKey => WEBCRYPTO_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::CryptoKey; lazy true,
    SubtleCrypto => WEBCRYPTO_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::SubtleCrypto; statics ["supports"],
    Event => NOOP_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::event_target::CLASS_ID_EVENT); statics ["NONE", "CAPTURING_PHASE", "AT_TARGET", "BUBBLING_PHASE"],
    MessageEvent => NOOP_CONSTRUCTOR_INFO; lazy true,
    CustomEvent => NOOP_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::event_target::CLASS_ID_CUSTOM_EVENT); lazy true,
    DOMException => NOOP_CONSTRUCTOR_INFO; instance BuiltinInstanceIdentity::Class(crate::event_target::CLASS_ID_DOM_EXCEPTION); statics ["INDEX_SIZE_ERR", "DOMSTRING_SIZE_ERR", "HIERARCHY_REQUEST_ERR", "WRONG_DOCUMENT_ERR", "INVALID_CHARACTER_ERR", "NO_DATA_ALLOWED_ERR", "NO_MODIFICATION_ALLOWED_ERR", "NOT_FOUND_ERR", "NOT_SUPPORTED_ERR", "INUSE_ATTRIBUTE_ERR", "INVALID_STATE_ERR", "SYNTAX_ERR", "INVALID_MODIFICATION_ERR", "NAMESPACE_ERR", "INVALID_ACCESS_ERR", "VALIDATION_ERR", "TYPE_MISMATCH_ERR", "SECURITY_ERR", "NETWORK_ERR", "ABORT_ERR", "URL_MISMATCH_ERR", "QUOTA_EXCEEDED_ERR", "TIMEOUT_ERR", "INVALID_NODE_TYPE_ERR", "DATA_CLONE_ERR"],
    FormData => NOOP_CONSTRUCTOR_INFO; lazy true,
    Blob => crate::fn_info!(#[realm(Blob, kind = constructor, provider = builtin_constructor, births = [ArrayBuffer, Promise, ReadableStream, Uint8Array], spec = {length = 0}, effects = { coerce(arg(0), Iterate); coerce(arg(0), ToPrimitive); coerce(elems(arg(0)), ToPrimitive); reads(arg(1)); coerce(field(arg(1), type), ToPrimitive); returns(fresh(Blob.prototype)) }, emit = metadata(realm_urlpattern_metadata))] export crate::object::global_this::fetch_globals::global_this_blob_thunk, 2; with_declared(2)); instance BuiltinInstanceIdentity::Class(0xFFFF0026); lazy true,
    File => crate::fn_info!(#[realm(File, kind = constructor, provider = builtin_constructor, births = [Blob], spec = {length = 2}, emit = metadata(realm_file_metadata))] export crate::object::global_this::fetch_globals::global_this_file_thunk, 3; with_declared(3)); instance BuiltinInstanceIdentity::Class(0xFFFF002F); lazy true,
    Headers => crate::fn_info!(#[realm(Headers, kind = constructor, provider = builtin_constructor, births = ["%IteratorPrototype%", Array], spec = {length = 0}, emit = metadata(realm_urlpattern_metadata))] export crate::object::global_this::fetch_globals::global_this_headers_thunk, 1; with_declared(1)); instance BuiltinInstanceIdentity::Class(0xFFFF002A); lazy true,
    Request => crate::fn_info!(#[realm(Request, kind = constructor, provider = builtin_constructor, births = [AbortSignal, ArrayBuffer, Blob, FormData, Headers, Promise, ReadableStream, URL], spec = {length = 1}, emit = metadata(realm_function_metadata), effects = { coerce(arg(0), ToPrimitive); reads(field(arg(1), method), field(arg(1), headers), field(arg(1), body), field(arg(1), referrer), field(arg(1), referrerPolicy), field(arg(1), mode), field(arg(1), credentials), field(arg(1), cache), field(arg(1), redirect), field(arg(1), integrity), field(arg(1), keepalive), field(arg(1), signal), field(arg(1), duplex), field(arg(1), priority), field(arg(1), window), field(arg(1), dispatcher)); coerce(field(arg(1), method), ToPrimitive); coerce(field(arg(1), referrer), ToPrimitive); coerce(field(arg(1), referrerPolicy), ToPrimitive); coerce(field(arg(1), mode), ToPrimitive); coerce(field(arg(1), credentials), ToPrimitive); coerce(field(arg(1), cache), ToPrimitive); coerce(field(arg(1), redirect), ToPrimitive); coerce(field(arg(1), integrity), ToPrimitive); coerce(field(arg(1), duplex), ToPrimitive); coerce(field(arg(1), priority), ToPrimitive); coerce(field(arg(1), headers), Iterate); coerce(elems(field(arg(1), headers)), Iterate); coerce(field(arg(1), headers), OwnKeys); coerce(elems(field(arg(1), headers)), ToPrimitive); escapes(field(arg(1), body)); stores(fresh); returns(fresh(Request.prototype, slots = {signal: field(arg(1), signal) | islot(arg(0), signal)})) })] export crate::object::global_this::fetch_globals::global_this_request_thunk, 2; with_declared(2)); instance BuiltinInstanceIdentity::Class(0xFFFF0029); lazy true,
    Response => crate::fn_info!(#[realm(Response, kind = constructor, provider = builtin_constructor, members = { error; json; redirect; }, spec = {length = 0}, emit = metadata(realm_urlpattern_metadata), births = [ArrayBuffer, Blob, FormData, Headers, Promise, ReadableStream], effects = { reads(field(arg(1), status), field(arg(1), statusText), field(arg(1), headers)); coerce(field(arg(1), status), ToPrimitive); coerce(field(arg(1), statusText), ToPrimitive); coerce(field(arg(1), headers), Iterate); coerce(elems(field(arg(1), headers)), Iterate); coerce(field(arg(1), headers), OwnKeys); coerce(elems(field(arg(1), headers)), ToPrimitive); escapes(arg(0)); stores(fresh); returns(fresh(Response.prototype, slots = {bodyError: unknown})) })] export crate::object::global_this::fetch_globals::global_this_response_thunk, 2; with_declared(2)); instance BuiltinInstanceIdentity::Class(0xFFFF0028); statics ["error", "json", "redirect"],
    MessageChannel => crate::fn_info!(#[realm(MessageChannel, kind = constructor, provider = builtin_constructor)] export crate::messaging_hook_slots::js_message_channel_constructor_call_error, 0; with_declared(0)); lazy true,
    MessagePort => crate::fn_info!(#[realm(MessagePort, kind = constructor, provider = builtin_constructor)] export crate::messaging_hook_slots::js_message_port_constructor_call_error, 0; with_declared(0)); lazy true,
    BroadcastChannel => crate::fn_info!(#[realm(BroadcastChannel, kind = constructor, provider = builtin_constructor, extends = EventTarget)] export crate::messaging_hook_slots::js_broadcast_channel_constructor_call_error, 1; with_declared(1)); lazy true,
    Storage => crate::fn_info!(#[realm(Storage, kind = constructor, provider = builtin_constructor)] export crate::web_storage::storage_constructor_illegal, 0; with_declared(0)); lazy true,
    WebSocket => NOOP_CONSTRUCTOR_INFO; statics ["CONNECTING", "OPEN", "CLOSING", "CLOSED"],
    FinalizationRegistry => NOOP_CONSTRUCTOR_INFO; class crate::weakref::CLASS_ID_FINALIZATION_REGISTRY; instance BuiltinInstanceIdentity::Class(crate::weakref::CLASS_ID_FINALIZATION_REGISTRY),
    // #2875: TC39 explicit-resource-management globals. Backed by the
    // no-op constructor thunk so `typeof DisposableStack === "function"`;
    // real `new DisposableStack()` / `new SuppressedError(...)` flow through
    // codegen's `lower_builtin_new` to the dedicated runtime ctors.
    DisposableStack => NOOP_CONSTRUCTOR_INFO; lazy true,
    AsyncDisposableStack => NOOP_CONSTRUCTOR_INFO; lazy true,
    SuppressedError => NOOP_CONSTRUCTOR_INFO; lazy true,
    // Abstract: `prototype` is the realm's %IteratorPrototype% (`iterator_ctor`).
    Iterator => CONSTRUCT_ONLY_INFO; statics ["from", "concat"],
    Buffer => crate::object::native_module::BUFFER_CONSTRUCTOR_INFO; statics ["from", "alloc", "allocUnsafe", "allocUnsafeSlow", "concat", "of", "isBuffer", "isEncoding", "byteLength", "compare", "copyBytesFrom", "poolSize"],
    // perf_hooks families (node: globals identical to the module exports),
    // born on first observation; their members are perf_hooks' own
    // (`perf_hook_slots::attach_perf_hooks_constructor`, in the birth).
    Performance => NOOP_CONSTRUCTOR_INFO; lazy true,
    PerformanceEntry => NOOP_CONSTRUCTOR_INFO; lazy true,
    PerformanceMark => crate::fn_info!(crate::object::global_this::fetch_globals::global_this_builtin_noop_thunk, 1; with_declared(realm_native_perf_hooks_PerformanceMark_metadata!(length))); lazy true,
    PerformanceMeasure => NOOP_CONSTRUCTOR_INFO; lazy true,
    PerformanceObserver => crate::fn_info!(crate::object::global_this::fetch_globals::global_this_builtin_noop_thunk, 1; with_declared(realm_native_perf_hooks_PerformanceObserver_metadata!(length))); lazy true; statics ["supportedEntryTypes"],
    PerformanceObserverEntryList => NOOP_CONSTRUCTOR_INFO; lazy true,
    PerformanceResourceTiming => NOOP_CONSTRUCTOR_INFO; lazy true,
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
        .any(|decl| decl.global && decl.name == name)
}

/// #3655: spec `length` (declared-parameter count) for each built-in
/// constructor, so `Ctor.length` reads the right arity through the runtime
/// value path (`const C = DataView; C.length === 1`) and
/// `Object.getOwnPropertyDescriptor(Ctor, 'length').value` matches Node. The
/// HIR also folds bare `Ctor.length` constants (`analysis::builtin_constructor_length`);
/// these are the runtime fallback for rebound / passed-as-value constructors.
/// Values verified against `node --experimental-strip-types`. Unlisted names
/// fall through to the closure arity registry (0).
#[rustfmt::skip] pub(crate) fn builtin_constructor_spec_length(name: &str) -> Option<u32> {
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
        | "ReadableStreamDefaultController"
        | "ReadableByteStreamController"
        | "ReadableStreamBYOBRequest"
        | "DisposableStack"
        | "AsyncDisposableStack"
        | "Iterator" => realm_urlpattern_metadata!(length),
        "CompressionStream"
        | "DecompressionStream"
        | "ReadableStreamDefaultReader"
        | "ReadableStreamBYOBReader" => 1,
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
        | "MessageEvent"
        | "Request"
        | "WebSocket"
        | "BroadcastChannel"
        | "FinalizationRegistry"
        | "Promise" => realm_function_metadata!(length),
        "RegExp" | "Proxy" | "AggregateError" | "File" => realm_file_metadata!(length),
        "Date" => realm_date_metadata!(length),
        "SuppressedError" | "Buffer" | "Uint8Array" | "Int8Array" | "Uint16Array"
        | "Int16Array" | "Uint32Array" | "Int32Array" | "Float16Array" | "Float32Array"
        | "Float64Array" | "Uint8ClampedArray" | "BigInt64Array" | "BigUint64Array" => realm_suppressederror_metadata!(length),
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

/// The call capture of a builtin constructor: its identity in the
/// runtime-internal 0x7FFE word space (DESIGN-p3 §1.2, R4), the same word the
/// realm image writes for `RealmValue::Int32` (`realm_image/atoms.rs`). It is
/// never a JS value, and never a Number (P3 B4a: nothing builds an INT32
/// Number).
pub(crate) const fn builtin_constructor_identity_word(identity: u32) -> u64 {
    crate::value::INT32_TAG | identity as u64
}

/// Birth consumes one declaration, so code, header identity and call capture
/// cannot disagree. The fact precedes publication and survives moving GC.
pub(crate) fn alloc_builtin_constructor(
    identity: BuiltinConstructorIdentity,
    realm: &crate::array::RealmIntrinsics,
) -> *mut crate::closure::ClosureHeader {
    let declaration = identity.declaration();
    let header = 2 | crate::closure::NO_THIS_REBIND_FLAG;
    let closure = crate::closure::js_closure_alloc(declaration.info, header);
    if !closure.is_null() {
        crate::closure::js_closure_set_capture_bits(
            closure,
            0,
            builtin_constructor_identity_word(identity as u32),
        );
        // Empty permanently denotes the main realm, including constructors
        // born before its global. A vm constructor keeps its own global alive.
        if !crate::array::is_thread_realm(realm) {
            let global = unsafe { crate::array::realm_record_global(realm) };
            crate::closure::js_closure_set_capture_bits(
                closure,
                1,
                crate::value::js_nanbox_pointer(global as i64).to_bits(),
            );
        }
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

/// The realm captured by a built-in constructor. Empty is the main realm's
/// permanent value; its global need not exist when the constructor is born.
///
/// # Safety
/// `ctor` is a live built-in constructor; no collection runs while the record is borrowed.
#[inline(always)]
pub(crate) unsafe fn builtin_constructor_realm<'a>(
    ctor: *const crate::closure::ClosureHeader,
) -> &'a crate::array::RealmIntrinsics {
    let global = crate::closure::js_closure_get_capture_bits(ctor, 1);
    if global == crate::value::TAG_UNDEFINED {
        crate::array::thread_realm()
    } else {
        super::global_this::realm_of((global & crate::value::POINTER_MASK) as *mut super::ObjectHeader)
    }
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

#[cfg(test)]
mod realm_info_export_tests {
    include!(concat!(env!("OUT_DIR"), "/realm_info_exports_test.rs"));
    use super::*;

    unsafe extern "C" fn compound_flags_body(
        _callee: *const crate::closure::ClosureHeader,
        _this: crate::codegen_abi::JsThis,
    ) -> f64 {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    }

    #[test]
    fn image_export_spells_compound_builders_as_tokens() {
        let info = crate::fn_info!(export crate::object::global_this_tables::realm_info_export_tests::compound_flags_body, 0;
            with_flags(crate::closure::FN_BUILTIN | crate::closure::FN_NON_CONSTRUCTOR),
            with_length({ let length = 3; length }), plain()
        );
        unsafe extern "C" {
            #[link_name = "perry_fn_info_perry-runtime::crate::object::global_this_tables::realm_info_export_tests::compound_flags_body::0::with_flags(crate :: closure :: FN_BUILTIN | crate :: closure :: FN_NON_CONSTRUCTOR );with_length({ let length = 3 ; length } );plain();"]
            static COMPOUND_INFO: crate::closure::JsFunctionInfo;
        }
        assert_eq!(std::ptr::addr_of!(COMPOUND_INFO), info);
        let info = unsafe { &*info };
        assert_eq!(info.length, 3);
        assert_eq!(info.plain_params(), 0);
        assert_eq!(
            info.flags,
            crate::closure::FN_BUILTIN
                | crate::closure::FN_NON_CONSTRUCTOR
                | crate::closure::FN_HAS_LENGTH
                // A runtime record owns its metadata entry (NativeFunctionInfo).
                | crate::codegen_abi::FN_HAS_METADATA
        );
    }

    #[test]
    fn image_names_the_same_info_as_the_constructor_birth() {
        unsafe extern "C" {
            #[link_name = "perry_fn_info_perry-runtime::crate::object::global_this::builtin_thunks::global_this_array_thunk::1::with_rest(0 );"]
            static ARRAY_INFO: crate::closure::JsFunctionInfo;
        }
        let born = BuiltinConstructorIdentity::Array.declaration().info;
        assert_eq!(std::ptr::addr_of!(ARRAY_INFO), born as *const _);
        assert_eq!(born.code, global_this_array_thunk as *const u8);
        assert_eq!(born.params, 1);
        assert_eq!(born.rest_fixed, 0);
    }
}
