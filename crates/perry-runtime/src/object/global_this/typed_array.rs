use super::*;

fn array_buffer_receiver_addr(this: crate::closure::JsThis) -> Option<usize> {
    let this_bits = this.bits();
    let this_jsv = JSValue::from_bits(this_bits);
    let raw = if this_jsv.is_pointer() {
        (this_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if this_bits >> 48 == 0
        && this_bits > 0x10000
        && crate::buffer::header_is_owned(this_bits as usize)
    {
        // #10694: a raw word must be allocator-owned before the brand read.
        this_bits as usize
    } else {
        return None;
    };
    if crate::buffer::is_registered_buffer(raw) && crate::buffer::is_array_buffer(raw) {
        Some(raw)
    } else {
        None
    }
}

fn array_buffer_brand_error() -> ! {
    super::super::object_ops::throw_object_type_error(
        b"Method get ArrayBuffer.prototype.byteLength called on incompatible receiver",
    )
}

pub(crate) extern "C" fn array_buffer_byte_length_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    match array_buffer_receiver_addr(this) {
        Some(addr) => {
            let buf = addr as *const crate::buffer::BufferHeader;
            f64::from_bits(
                crate::value::JSValue::number(crate::buffer::js_buffer_length(buf) as f64).bits(),
            )
        }
        None => array_buffer_brand_error(),
    }
}

/// Receiver-address resolver for the `SharedArrayBuffer.prototype.byteLength`
/// getter. Mirrors `array_buffer_receiver_addr` but accepts only buffers in the
/// shared registry, so the getter rejects a plain `ArrayBuffer` `this`
/// (test262 SharedArrayBuffer/prototype/byteLength/this-is-arraybuffer).
fn shared_array_buffer_receiver_addr(this: crate::closure::JsThis) -> Option<usize> {
    let this_bits = this.bits();
    let this_jsv = JSValue::from_bits(this_bits);
    let raw = if this_jsv.is_pointer() {
        (this_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if this_bits >> 48 == 0
        && this_bits > 0x10000
        && crate::buffer::header_is_owned(this_bits as usize)
    {
        // #10694: a raw word must be allocator-owned before the brand read.
        this_bits as usize
    } else {
        return None;
    };
    if crate::buffer::is_registered_buffer(raw) && crate::buffer::is_shared_array_buffer(raw) {
        Some(raw)
    } else {
        None
    }
}

pub(crate) extern "C" fn shared_array_buffer_byte_length_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    match shared_array_buffer_receiver_addr(this) {
        Some(addr) => {
            let buf = addr as *const crate::buffer::BufferHeader;
            f64::from_bits(
                crate::value::JSValue::number(crate::buffer::js_buffer_length(buf) as f64).bits(),
            )
        }
        None => super::super::object_ops::throw_object_type_error(
            b"Method get SharedArrayBuffer.prototype.byteLength called on incompatible receiver",
        ),
    }
}

/// `SharedArrayBuffer.prototype.slice(start, end)`. The brand check (the `this`
/// value must be a SharedArrayBuffer, never a plain ArrayBuffer or a
/// non-object) lives here so `SharedArrayBuffer.prototype.slice.call(notSab)`
/// throws a TypeError; the byte copy + ToIntegerOrInfinity arg coercion is
/// `buffer_dispatch::array_buffer_slice`, shared with `ArrayBuffer`'s.
pub(crate) extern "C" fn shared_array_buffer_slice_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    start: f64,
    end: f64,
) -> f64 {
    match shared_array_buffer_receiver_addr(this) {
        Some(addr) => unsafe {
            super::super::buffer_dispatch::array_buffer_slice(addr, &[start, end])
        },
        None => super::super::object_ops::throw_object_type_error(
            b"Method SharedArrayBuffer.prototype.slice called on incompatible receiver",
        ),
    }
}

pub(crate) extern "C" fn array_buffer_slice_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    start: f64,
    end: f64,
) -> f64 {
    match array_buffer_receiver_addr(this) {
        Some(addr) => unsafe {
            super::super::buffer_dispatch::array_buffer_slice(addr, &[start, end])
        },
        None => super::super::object_ops::throw_object_type_error(
            b"Method ArrayBuffer.prototype.slice called on incompatible receiver",
        ),
    }
}

/// `ArrayBuffer.prototype.{resize, transfer, transferToFixedLength}`: brand
/// check (a plain ArrayBuffer, never a SharedArrayBuffer or a DataView), then
/// the implementation. Instance calls reach these through the prototype member.
fn array_buffer_method(
    this: crate::closure::JsThis,
    method: &'static str,
    arg: f64,
    brand_error: &[u8],
) -> f64 {
    match array_buffer_receiver_addr(this) {
        Some(addr)
            if !crate::buffer::is_shared_array_buffer(addr)
                && !crate::buffer::is_data_view(addr) =>
        {
            let args = [arg];
            if method == "resize" {
                crate::buffer::array_buffer_resize(addr, &args)
            } else {
                crate::buffer::array_buffer_transfer(addr, &args, method == "transfer")
            }
        }
        _ => super::super::object_ops::throw_object_type_error(brand_error),
    }
}

pub(crate) extern "C" fn array_buffer_resize_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    new_length: f64,
) -> f64 {
    array_buffer_method(
        this,
        "resize",
        new_length,
        b"Method ArrayBuffer.prototype.resize called on incompatible receiver",
    )
}

pub(crate) extern "C" fn array_buffer_transfer_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    new_length: f64,
) -> f64 {
    array_buffer_method(
        this,
        "transfer",
        new_length,
        b"Method ArrayBuffer.prototype.transfer called on incompatible receiver",
    )
}

pub(crate) extern "C" fn array_buffer_transfer_to_fixed_length_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    new_length: f64,
) -> f64 {
    array_buffer_method(
        this,
        "transferToFixedLength",
        new_length,
        b"Method ArrayBuffer.prototype.transferToFixedLength called on incompatible receiver",
    )
}

/// `SharedArrayBuffer.prototype.grow(newLength)` (length 1).
pub(crate) extern "C" fn shared_array_buffer_grow_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    new_length: f64,
) -> f64 {
    match shared_array_buffer_receiver_addr(this) {
        Some(addr) => crate::buffer::shared_array_buffer_grow(addr, &[new_length]),
        None => super::super::object_ops::throw_object_type_error(
            b"Method SharedArrayBuffer.prototype.grow called on incompatible receiver",
        ),
    }
}

/// `get SharedArrayBuffer.prototype.growable` / `maxByteLength`.
fn shared_array_buffer_flag_getter(this: crate::closure::JsThis, key: &[u8]) -> f64 {
    let Some(addr) = shared_array_buffer_receiver_addr(this) else {
        super::super::object_ops::throw_object_type_error(if key == b"growable" {
            b"Method get SharedArrayBuffer.prototype.growable called on incompatible receiver"
        } else {
            b"Method get SharedArrayBuffer.prototype.maxByteLength called on incompatible receiver"
        })
    };
    let value = if key == b"growable" {
        JSValue::bool(crate::buffer::is_resizable_buffer(addr))
    } else {
        let buf = addr as *const crate::buffer::BufferHeader;
        JSValue::number(
            crate::buffer::resizable_max_byte_length(addr)
                .map(|max| max as f64)
                .unwrap_or_else(|| crate::buffer::js_buffer_length(buf) as f64),
        )
    };
    f64::from_bits(value.bits())
}

pub(crate) extern "C" fn shared_array_buffer_growable_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    shared_array_buffer_flag_getter(this, b"growable")
}

pub(crate) extern "C" fn shared_array_buffer_max_byte_length_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    shared_array_buffer_flag_getter(this, b"maxByteLength")
}

/// Shared body of the reflectable `resizable` / `maxByteLength` / `detached`
/// accessors on `ArrayBuffer.prototype`. Instance reads are answered by
/// `get_field_by_name_tail`; this is the `Object.getOwnPropertyDescriptor(
/// ArrayBuffer.prototype, "resizable").get.call(ab)` route.
fn array_buffer_flag_getter(this: crate::closure::JsThis, key: &[u8]) -> f64 {
    let Some(addr) = array_buffer_receiver_addr(this) else {
        super::super::object_ops::throw_object_type_error(
            b"Method ArrayBuffer.prototype getter called on incompatible receiver",
        )
    };
    let value = match key {
        b"resizable" => JSValue::bool(crate::buffer::is_resizable_buffer(addr)),
        b"detached" => JSValue::bool(crate::buffer::is_detached_buffer(addr)),
        _ => {
            let buf = addr as *const crate::buffer::BufferHeader;
            let max = if crate::buffer::is_detached_buffer(addr) {
                0.0
            } else {
                crate::buffer::resizable_max_byte_length(addr)
                    .map(|max| max as f64)
                    .unwrap_or_else(|| crate::buffer::js_buffer_length(buf) as f64)
            };
            JSValue::number(max)
        }
    };
    f64::from_bits(value.bits())
}

pub(crate) extern "C" fn array_buffer_resizable_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    array_buffer_flag_getter(this, b"resizable")
}

pub(crate) extern "C" fn array_buffer_max_byte_length_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    array_buffer_flag_getter(this, b"maxByteLength")
}

pub(crate) extern "C" fn array_buffer_detached_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    array_buffer_flag_getter(this, b"detached")
}

pub(crate) unsafe fn validate_array_buffer_species_constructor(addr: usize) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(addr as i64));
    let receiver_value = &receiver;
    let receiver_addr =
        || crate::value::js_nanbox_get_pointer(receiver_value.get_nanbox_f64()) as usize;

    // SpeciesConstructor starts with ordinary Get(O, "constructor"). Buffer
    // own data/accessor properties live in side tables; absent own state falls
    // through the receiver's recorded custom prototype chain before using the
    // intrinsic constructor.
    let value = if let Some(accessor) =
        super::super::get_accessor_descriptor(receiver_addr(), "constructor")
    {
        if accessor.get == 0 {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        } else {
            f64::from_bits(
                super::super::invoke_accessor_getter(accessor.get, receiver_value.get_nanbox_f64())
                    .bits(),
            )
        }
    } else if let Some(value) = crate::buffer::buffer_get_own_prop(receiver_addr(), "constructor") {
        value
    } else {
        let inherited =
            if super::super::prototype_chain::object_static_prototype(receiver_addr()).is_some() {
                let key = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
                super::super::prototype_chain::resolve_inherited_field(receiver_addr(), key)
                    .map(|value| f64::from_bits(value.bits()))
            } else {
                None
            };
        inherited.unwrap_or_else(|| {
            super::builtin_constructor_value(
                if crate::buffer::is_shared_array_buffer(receiver_addr()) {
                    "SharedArrayBuffer"
                } else {
                    "ArrayBuffer"
                },
            )
        })
    };
    let value = JSValue::from_bits(value.to_bits());
    // `null` is NaN-boxed in the pointer-shaped value family, but it is not
    // an Object and therefore cannot supply @@species.
    if !value.is_undefined()
        && (value.is_null()
            || (!value.is_pointer()
                && !super::super::js_value_is_constructor(f64::from_bits(value.bits())))
            // Symbols use the pointer-shaped NaN-box family, but remain
            // primitives for SpeciesConstructor's Type(C) check.
            || crate::symbol::js_is_symbol(f64::from_bits(value.bits())) != 0)
    {
        super::super::object_ops::throw_object_type_error(
            b"ArrayBuffer species constructor is not an object",
        );
    }
    if value.is_undefined() {
        return;
    }

    // Complete SpeciesConstructor validation: Get(C, @@species), accept
    // undefined/null as the default constructor, otherwise require an actual
    // constructable value. The constructor is rooted because a species getter
    // can execute arbitrary user code and collect.
    let constructor = scope.root_nanbox_f64(f64::from_bits(value.bits()));
    let species = crate::symbol::well_known_symbol("species");
    if species.is_null() {
        return;
    }
    let species_value = crate::symbol::js_object_get_symbol_property(
        constructor.get_nanbox_f64(),
        f64::from_bits(JSValue::pointer(species as *const u8).bits()),
    );
    let species_value = JSValue::from_bits(species_value.to_bits());
    if species_value.is_undefined() || species_value.is_null() {
        return;
    }
    if !super::super::js_value_is_constructor(f64::from_bits(species_value.bits())) {
        super::super::object_ops::throw_object_type_error(
            b"ArrayBuffer species is not a constructor",
        );
    }
}

pub(crate) extern "C" fn array_buffer_is_view_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    value: f64,
) -> f64 {
    // #11239: the same classifier `util.types.isArrayBufferView` and the
    // direct-call lowering answer from, so `const f = ArrayBuffer.isView`
    // cannot disagree with `ArrayBuffer.isView(x)` (it did for a Buffer).
    let is_view = crate::object::view_brand::view_brand(value).is_some();
    f64::from_bits(crate::value::JSValue::bool(is_view).bits())
}

#[derive(Clone, Copy)]
enum TypedArrayAccessorReceiver {
    Registered(*const crate::typedarray_hook_slots::TypedArrayHeader, u8),
    Uint8Buffer(usize),
}

impl TypedArrayAccessorReceiver {
    fn length(self) -> u32 {
        match self {
            Self::Registered(ta, _) => {
                crate::typedarray_hook_slots::js_typed_array_length(ta).max(0) as u32
            }
            Self::Uint8Buffer(addr) => {
                crate::buffer::js_buffer_length(addr as *const crate::buffer::BufferHeader).max(0)
                    as u32
            }
        }
    }

    fn byte_length(self) -> usize {
        match self {
            Self::Registered(_, kind) => {
                self.length() as usize * crate::typedarray_hook_slots::elem_size_for_kind(kind)
            }
            Self::Uint8Buffer(_) => self.length() as usize,
        }
    }

    fn byte_offset(self) -> u32 {
        match self {
            Self::Registered(ta, _) => crate::typedarray_view::js_typed_array_byte_offset(ta),
            Self::Uint8Buffer(addr) => crate::buffer::buffer_byte_offset(addr),
        }
    }

    fn backing_buffer(self) -> usize {
        match self {
            Self::Registered(ta, _) => {
                crate::typedarray_view::js_typed_array_backing_buffer(ta) as usize
            }
            Self::Uint8Buffer(addr) => crate::buffer::buffer_backing_array_buffer(addr),
        }
    }
}

/// Resolve the `this` receiver for the reflected
/// `%TypedArray%.prototype` accessors installed by #2060. A Perry
/// `Uint8Array` (and Node's `Buffer` subclass) uses `BufferHeader`, so a miss
/// in `lookup_typed_array_kind` must continue through the strict buffer brand
/// check instead of becoming a `TypeError` (#9347).
fn typed_array_receiver(this: crate::closure::JsThis) -> Option<TypedArrayAccessorReceiver> {
    use crate::value::JSValue;
    let this_bits = this.bits();
    let this_jsv = JSValue::from_bits(this_bits);
    let raw = if this_jsv.is_pointer() {
        (this_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if this_bits >> 48 == 0
        && this_bits > 0x10000
        && crate::buffer::header_is_owned(this_bits as usize)
    {
        // #10694: a raw word must be allocator-owned before the brand read.
        this_bits as usize
    } else {
        return None;
    };
    if let Some(kind) = crate::typedarray_hook_slots::lookup_typed_array_kind(raw) {
        return Some(TypedArrayAccessorReceiver::Registered(
            raw as *const _,
            kind,
        ));
    }
    crate::object::typed_array_proto_thunks::is_typed_array_buffer(raw)
        .then_some(TypedArrayAccessorReceiver::Uint8Buffer(raw))
}

fn typed_array_brand_error() -> ! {
    super::super::object_ops::throw_object_type_error(
        b"Method get %TypedArray%.prototype accessor called on incompatible receiver",
    )
}

fn string_value_to_owned(value: f64) -> Option<String> {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if !jv.is_any_string() {
        return None;
    }
    let s = crate::builtins::js_string_coerce(value);
    if s.is_null() {
        return None;
    }
    unsafe {
        let bytes = (s as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let len = (*s).byte_len as usize;
        std::str::from_utf8(std::slice::from_raw_parts(bytes, len))
            .ok()
            .map(ToOwned::to_owned)
    }
}

pub(crate) fn typed_array_constructor_this_kind(this: crate::closure::JsThis) -> Option<u8> {
    let this_value = f64::from_bits(this.bits());
    let ptr = crate::value::js_nanbox_get_pointer(this_value) as usize;
    if ptr == 0 || !crate::closure::is_closure_ptr(ptr) {
        return None;
    }
    let name_value = crate::closure::closure_get_dynamic_prop(ptr, "name");
    let name = string_value_to_owned(f64::from_bits(name_value.to_bits()))?;
    crate::typedarray_hook_slots::kind_for_name(&name)
}

fn typed_array_buffer_value(receiver: TypedArrayAccessorReceiver) -> f64 {
    let backing = receiver.backing_buffer();
    if backing == 0 {
        typed_array_brand_error();
    }
    crate::value::js_nanbox_pointer(backing as i64)
}

/// `%TypedArray%.prototype.length` getter — element count of the receiver.
extern "C" fn typed_array_length_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    match typed_array_receiver(this) {
        Some(receiver) => {
            f64::from_bits(crate::value::JSValue::number(receiver.length() as f64).bits())
        }
        None => typed_array_brand_error(),
    }
}

/// `%TypedArray%.prototype.byteLength` getter — `length * BYTES_PER_ELEMENT`.
extern "C" fn typed_array_byte_length_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    match typed_array_receiver(this) {
        Some(receiver) => {
            f64::from_bits(crate::value::JSValue::number(receiver.byte_length() as f64).bits())
        }
        None => typed_array_brand_error(),
    }
}

/// `%TypedArray%.prototype.byteOffset` getter.
extern "C" fn typed_array_byte_offset_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    match typed_array_receiver(this) {
        Some(receiver) => {
            f64::from_bits(crate::value::JSValue::number(receiver.byte_offset() as f64).bits())
        }
        None => typed_array_brand_error(),
    }
}

/// `%TypedArray%.prototype.buffer` getter, with the same stable backing
/// identity as the direct property path.
extern "C" fn typed_array_buffer_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    match typed_array_receiver(this) {
        Some(receiver) => typed_array_buffer_value(receiver),
        None => typed_array_brand_error(),
    }
}

/// `%TypedArray%.prototype [ @@toStringTag ]` getter (ES2024 23.2.3.38). When
/// `this` is a TypedArray it returns the constructor name (`"Int8Array"`,
/// `"Uint8Array"`, …); for any other receiver it returns `undefined` (NO
/// throw — the spec getter is `undefined`-tolerant). `safe-stable-stringify`
/// (a pino dependency) detects typed arrays via
/// `getOwnPropertyDescriptor(%TypedArray%.prototype, Symbol.toStringTag).get`
/// then `desc.get.call(value)`, so a missing accessor previously threw
/// `Cannot read properties of undefined (reading 'get')`.
extern "C" fn typed_array_to_string_tag_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    let this = f64::from_bits(this.bits());
    match crate::object::typed_array_to_string_tag_name(this) {
        Some(name) => {
            let s = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            f64::from_bits(crate::js_nanbox_string(s as i64).to_bits())
        }
        None => f64::from_bits(crate::value::TAG_UNDEFINED),
    }
}

/// Install the `%TypedArray%.prototype [ @@toStringTag ]` accessor (get-only,
/// `{ enumerable: false, configurable: true }`) on the intrinsic prototype so
/// `Object.getOwnPropertyDescriptor(%TypedArray%.prototype, Symbol.toStringTag)`
/// reflects a real accessor descriptor with a callable `.get`. The getter's
/// `this`-based result drives `safe-stable-stringify`'s typed-array detection.
fn install_typed_array_to_string_tag(proto_obj: *mut ObjectHeader) {
    if proto_obj.is_null() {
        return;
    }
    let sym = crate::symbol::well_known_symbol("toStringTag");
    if sym.is_null() {
        return;
    }
    unsafe {
        let f = crate::fn_info!(#[realm("%TypedArray%.prototype[\"@@toStringTag\"].<get>", constructible = true)] export crate::object::global_this::typed_array::typed_array_to_string_tag_getter_thunk, 0; with_declared(0));
        let c = crate::closure::js_closure_alloc(f, 0);
        if c.is_null() {
            return;
        }
        super::super::native_module::set_bound_native_closure_name(c, "get [Symbol.toStringTag]");
        let get_bits = crate::value::js_nanbox_pointer(c as i64).to_bits();
        let proto_value = crate::value::js_nanbox_pointer(proto_obj as i64);
        let sym_value = f64::from_bits(crate::value::JSValue::pointer(sym as *const u8).bits());
        crate::symbol::set_symbol_accessor_property(proto_value, sym_value, get_bits, 0);
        crate::symbol::set_symbol_property_attrs(
            proto_obj as usize,
            sym as usize,
            super::super::PropertyAttrs::new(false, false, true),
        );
    }
}

/// Install the four `%TypedArray%.prototype` accessor descriptors
/// (`buffer`, `byteLength`, `byteOffset`, `length`: node's own-key order) on a typed-array
/// constructor's prototype object so `Object.getOwnPropertyDescriptor`
/// reflects them as `{ get, set: undefined, enumerable: false,
/// configurable: true }`. #2060.
fn install_typed_array_proto_accessors(proto_obj: *mut ObjectHeader) {
    unsafe {
        // 0-arg getters: `.call(this)` forwards 0 user args.
        let mk = |f: *const crate::closure::JsFunctionInfo| -> u64 {
            let c = crate::closure::js_closure_alloc(f, 0);
            if c.is_null() {
                0
            } else {
                crate::value::js_nanbox_pointer(c as i64).to_bits()
            }
        };
        install_builtin_getter(
            proto_obj,
            "buffer",
            mk(crate::fn_info!(#[realm("%TypedArray%.prototype.buffer.<get>", provider = intrinsic_closure, constructible = true)] export crate::object::global_this::typed_array::typed_array_buffer_getter_thunk, 0; with_declared(0))),
        );
        install_builtin_getter(
            proto_obj,
            "byteLength",
            mk(crate::fn_info!(#[realm("%TypedArray%.prototype.byteLength.<get>", provider = intrinsic_closure, constructible = true)] export crate::object::global_this::typed_array::typed_array_byte_length_getter_thunk, 0; with_declared(0))),
        );
        install_builtin_getter(
            proto_obj,
            "byteOffset",
            mk(crate::fn_info!(#[realm("%TypedArray%.prototype.byteOffset.<get>", provider = intrinsic_closure, constructible = true)] export crate::object::global_this::typed_array::typed_array_byte_offset_getter_thunk, 0; with_declared(0))),
        );
        install_builtin_getter(
            proto_obj,
            "length",
            mk(crate::fn_info!(#[realm("%TypedArray%.prototype.length.<get>", provider = intrinsic_closure, constructible = true)] export crate::object::global_this::typed_array::typed_array_length_getter_thunk, 0; with_declared(0))),
        );
    }
}

/// Install `%Function.prototype% [ @@hasInstance ]` (#3662). Pre-fix this was
/// `undefined` — `typeof Function.prototype[Symbol.hasInstance]` reported
/// "undefined", a reflective `.call` threw, and a class with a custom
/// `static [Symbol.hasInstance]` was the only way to reach the protocol. The
/// method is keyed by the real well-known `Symbol.hasInstance` (not an
/// `@@`-string own property, which would leak into `getOwnPropertyNames`).
pub(crate) fn install_function_has_instance_symbol(proto_obj: *mut ObjectHeader) {
    if proto_obj.is_null() {
        return;
    }
    unsafe {
        let func_ptr = crate::fn_info!(#[realm("Function.prototype[\"@@hasInstance\"]", constructible = true)] export crate::object::instanceof::function_prototype_has_instance_thunk, 1; with_declared(1));
        let closure = crate::closure::js_closure_alloc(func_ptr, 0);
        if closure.is_null() {
            return;
        }
        super::super::native_module::set_bound_native_closure_name(closure, "[Symbol.hasInstance]");
        super::super::native_module::set_builtin_closure_length(closure as usize, 1);
        let sym = crate::symbol::well_known_symbol("hasInstance");
        if sym.is_null() {
            return;
        }
        let proto_value = crate::value::js_nanbox_pointer(proto_obj as i64);
        let sym_value = f64::from_bits(crate::value::JSValue::pointer(sym as *const u8).bits());
        let fn_value = f64::from_bits(crate::value::js_nanbox_pointer(closure as i64).to_bits());
        crate::symbol::js_object_set_symbol_property(proto_value, sym_value, fn_value);
    }
}

fn install_typed_array_iterator_symbol(proto_obj: *mut ObjectHeader) {
    if proto_obj.is_null() {
        return;
    }
    // Read the already-installed `values` method (installed by
    // `install_typed_array_proto_methods` just before this call) and bind it as
    // `@@iterator`. ECMA-262 §23.2.3.35: `%TypedArray%.prototype[@@iterator]`
    // is the same function object as `%TypedArray%.prototype.values`.
    unsafe {
        let values_key = crate::string::js_string_from_bytes(b"values".as_ptr(), 6);
        let values = js_object_get_field_by_name(proto_obj, values_key);
        let iter = crate::symbol::well_known_symbol("iterator");
        if !iter.is_null() && values.bits() != crate::value::TAG_UNDEFINED {
            let proto_value = crate::value::js_nanbox_pointer(proto_obj as i64);
            let iter_value =
                f64::from_bits(crate::value::JSValue::pointer(iter as *const u8).bits());
            crate::symbol::js_object_set_symbol_property(
                proto_value,
                iter_value,
                f64::from_bits(values.bits()),
            );
            crate::symbol::set_symbol_property_attrs(
                proto_obj as usize,
                iter as usize,
                crate::object::PropertyAttrs::new(true, false, true),
            );
        }
    }
}

/// `%TypedArray%.prototype`'s own members, installed by the `%TypedArray%`
/// family's birth (`birth_family`, after its `constructor`). The per-kind
/// prototypes (`Int8Array.prototype`, ...) inherit them through their declared
/// `extends`, so `Int8Array.prototype.map === %TypedArray%.prototype.map` and
/// the brand checks fire when a method is read off the parent (test262's
/// `testTypedArray.js`). Order is node's own-key order: the four accessors
/// (#2060), the spec methods, `toString` (= `Array.prototype.toString`),
/// `@@iterator` (= `values`, so after the methods) and `@@toStringTag`.
pub(crate) fn install_typed_array_prototype_members(realm: &crate::array::RealmIntrinsics, proto: *mut ObjectHeader) {
    install_typed_array_proto_accessors(proto);
    install_typed_array_to_string_tag(proto);
    typed_array_proto_thunks::install_typed_array_proto_methods(proto);
    install_typed_array_iterator_symbol(proto);
    super::populate::alias_typed_array_proto_to_string(realm, proto);
}

/// Public accessor for the `%TypedArray%.prototype` object. Returns the cached
/// pointer if `populate_global_this_builtins` has run (so the intrinsic is
/// initialised), else null. Used by `js_object_get_prototype_of` to resolve
/// `Object.getPrototypeOf(Int8Array.prototype)` to the shared prototype.
pub(crate) fn typed_array_intrinsic_proto_ptr() -> *mut ObjectHeader {
    crate::array::intrinsic_prototype_addr_if_bound(crate::object::BuiltinConstructorIdentity::TypedArray)
        as *mut ObjectHeader
}

// ---------------------------------------------------------------------------
// #3664: generator / async-generator intrinsic prototype towers.
// ---------------------------------------------------------------------------

/// The original Uint8Array prototype is owned by the traced intrinsic
/// constructor, independent of writable globalThis.Uint8Array.
pub(crate) fn typed_array_uint8_intrinsic_prototype_value() -> f64 {
    // Uint8Array's family record: its original prototype.
    let proto = crate::array::intrinsic_prototype_addr(
        crate::object::BuiltinConstructorIdentity::Uint8Array,
    );
    if proto == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    crate::value::js_nanbox_pointer(proto as i64)
}
