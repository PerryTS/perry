use super::super::*;
use super::*;

fn array_buffer_receiver_addr() -> Option<usize> {
    let this_bits = IMPLICIT_THIS.with(|c| c.get());
    let this_jsv = JSValue::from_bits(this_bits);
    let raw = if this_jsv.is_pointer() {
        (this_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if this_bits >> 48 == 0 && this_bits > 0x10000 {
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
) -> f64 {
    match array_buffer_receiver_addr() {
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
fn shared_array_buffer_receiver_addr() -> Option<usize> {
    let this_bits = IMPLICIT_THIS.with(|c| c.get());
    let this_jsv = JSValue::from_bits(this_bits);
    let raw = if this_jsv.is_pointer() {
        (this_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if this_bits >> 48 == 0 && this_bits > 0x10000 {
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
) -> f64 {
    match shared_array_buffer_receiver_addr() {
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
/// throws a TypeError; the actual byte copy + ToIntegerOrInfinity arg coercion
/// is shared with the instance dispatch in `buffer_dispatch`.
pub(crate) extern "C" fn shared_array_buffer_slice_thunk(
    _closure: *const crate::closure::ClosureHeader,
    start: f64,
    end: f64,
) -> f64 {
    match shared_array_buffer_receiver_addr() {
        Some(addr) => unsafe {
            let args = [start, end];
            super::super::buffer_dispatch::dispatch_buffer_method(addr, "slice", args.as_ptr(), 2)
        },
        None => super::super::object_ops::throw_object_type_error(
            b"Method SharedArrayBuffer.prototype.slice called on incompatible receiver",
        ),
    }
}

pub(crate) extern "C" fn array_buffer_is_view_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let jv = JSValue::from_bits(value.to_bits());
    let addr = if jv.is_pointer() {
        (value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if value.to_bits() >> 48 == 0 && value.to_bits() > 0x10000 {
        value.to_bits() as usize
    } else {
        0
    };
    let is_view = (addr != 0
        && !crate::buffer::is_any_array_buffer(addr)
        && (crate::buffer::is_uint8array_buffer(addr) || crate::buffer::is_data_view(addr)))
        || jsvalue_extends_data_view(value)
        || crate::typedarray::lookup_typed_array_kind(addr).is_some();
    f64::from_bits(crate::value::JSValue::bool(is_view).bits())
}

fn jsvalue_extends_data_view(value: f64) -> bool {
    let v = JSValue::from_bits(value.to_bits());
    if !v.is_pointer() {
        return false;
    }
    let ptr = v.as_pointer::<u8>();
    if ptr.is_null() || !crate::object::is_valid_obj_ptr(ptr) {
        return false;
    }
    unsafe {
        let gc_header = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT {
            return false;
        }
        let obj = ptr as *const ObjectHeader;
        let class_id = (*obj).class_id;
        class_id != 0 && crate::object::extends_builtin_data_view(class_id)
    }
}

/// Resolve the `IMPLICIT_THIS` receiver to a `(typed-array ptr, kind)` if it
/// is a typed array, else `None`. Backs the `%TypedArray%.prototype` accessor
/// getters installed for reflection (#2060) — these fire when user code does
/// `desc.get.call(int8arr)` after pulling the descriptor out via
/// `Object.getOwnPropertyDescriptor`. Mirrors the receiver-extraction the
/// `Array.prototype.slice` thunk uses (NaN-boxed pointer or raw-i64 form).
fn typed_array_receiver() -> Option<(*const crate::typedarray::TypedArrayHeader, u8)> {
    use crate::value::JSValue;
    let this_bits = IMPLICIT_THIS.with(|c| c.get());
    let this_jsv = JSValue::from_bits(this_bits);
    let raw = if this_jsv.is_pointer() {
        (this_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if this_bits >> 48 == 0 && this_bits > 0x10000 {
        this_bits as usize
    } else {
        return None;
    };
    let kind = crate::typedarray::lookup_typed_array_kind(raw)?;
    Some((raw as *const crate::typedarray::TypedArrayHeader, kind))
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

pub(crate) fn typed_array_constructor_this_kind() -> Option<u8> {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    let ptr = crate::value::js_nanbox_get_pointer(this_value) as usize;
    if ptr == 0 || !crate::closure::is_closure_ptr(ptr) {
        return None;
    }
    let name_value = crate::closure::closure_get_dynamic_prop(ptr, "name");
    let name = string_value_to_owned(f64::from_bits(name_value.to_bits()))?;
    crate::typedarray::kind_for_name(&name)
}

fn typed_array_buffer_value(ta: *const crate::typedarray::TypedArrayHeader) -> f64 {
    let buf = crate::typedarray::typed_array_to_array_buffer(ta);
    if buf.is_null() {
        typed_array_brand_error();
    }
    crate::value::js_nanbox_pointer(buf as i64)
}

/// `%TypedArray%.prototype.length` getter — element count of the receiver.
extern "C" fn typed_array_length_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    match typed_array_receiver() {
        Some((ta, _)) => {
            let len = crate::typedarray::js_typed_array_length(ta);
            f64::from_bits(crate::value::JSValue::number(len as f64).bits())
        }
        None => typed_array_brand_error(),
    }
}

/// `%TypedArray%.prototype.byteLength` getter — `length * BYTES_PER_ELEMENT`.
extern "C" fn typed_array_byte_length_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    match typed_array_receiver() {
        Some((ta, kind)) => {
            let len = crate::typedarray::js_typed_array_length(ta) as usize;
            let elem_size = crate::typedarray::elem_size_for_kind(kind);
            f64::from_bits(crate::value::JSValue::number((len * elem_size) as f64).bits())
        }
        None => typed_array_brand_error(),
    }
}

/// `%TypedArray%.prototype.byteOffset` getter — always 0 (Perry views are not
/// backed by an offset into a shared `ArrayBuffer`).
extern "C" fn typed_array_byte_offset_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    match typed_array_receiver() {
        Some(_) => f64::from_bits(crate::value::JSValue::number(0.0).bits()),
        None => typed_array_brand_error(),
    }
}

/// `%TypedArray%.prototype.buffer` getter. Perry does not yet model a
/// first-class `ArrayBuffer` behind a view, so this returns `undefined` for
/// now (matching the existing `int8arr.buffer` data-path behavior). The
/// accessor still exists so reflection sees a real getter — closing the
/// `getOwnPropertyDescriptor(...).get` cascade in #2060.
extern "C" fn typed_array_buffer_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    match typed_array_receiver() {
        Some((ta, _)) => typed_array_buffer_value(ta),
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
) -> f64 {
    let this = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
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
        let f = typed_array_to_string_tag_getter_thunk as *const u8;
        crate::closure::js_register_closure_arity(f, 0);
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
/// (`length`, `byteLength`, `byteOffset`, `buffer`) on a typed-array
/// constructor's prototype object so `Object.getOwnPropertyDescriptor`
/// reflects them as `{ get, set: undefined, enumerable: false,
/// configurable: true }`. #2060.
fn install_typed_array_proto_accessors(proto_obj: *mut ObjectHeader) {
    unsafe {
        // 0-arg getters: `.call(this)` forwards 0 user args.
        let mk = |f: *const u8| -> u64 {
            crate::closure::js_register_closure_arity(f, 0);
            let c = crate::closure::js_closure_alloc(f, 0);
            if c.is_null() {
                0
            } else {
                crate::value::js_nanbox_pointer(c as i64).to_bits()
            }
        };
        install_builtin_getter(
            proto_obj,
            "length",
            mk(typed_array_length_getter_thunk as *const u8),
        );
        install_builtin_getter(
            proto_obj,
            "byteLength",
            mk(typed_array_byte_length_getter_thunk as *const u8),
        );
        install_builtin_getter(
            proto_obj,
            "byteOffset",
            mk(typed_array_byte_offset_getter_thunk as *const u8),
        );
        install_builtin_getter(
            proto_obj,
            "buffer",
            mk(typed_array_buffer_getter_thunk as *const u8),
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
        let func_ptr = super::super::instanceof::function_prototype_has_instance_thunk as *const u8;
        crate::closure::js_register_closure_arity(func_ptr, 1);
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
        }
    }
}

/// Allocate the shared `%TypedArray%` intrinsic constructor (a closure) and
/// its `.prototype` object, cache both in the GC-rooted atomics, and wire the
/// closure's `prototype` dynamic-prop to point at the shared prototype.
///
/// Spec: `%TypedArray%` is the abstract parent constructor for `Int8Array`,
/// `Uint8Array`, … — `Int8Array.__proto__ === %TypedArray%` and
/// `Object.getPrototypeOf(Int8Array.prototype) === %TypedArray%.prototype`.
/// Perry didn't model this before #2145, so test262's TypedArray-prototype
/// walks read `null.prototype` and the constructor's `__proto__` returned the
/// `0.0` no-value placeholder (`typeof Int8Array.__proto__ === "number"`).
///
/// Idempotent: subsequent calls return the cached pointer. Called from
/// `populate_global_this_builtins` (single-threaded under the singleton CAS),
/// so the AtomicI64 stores don't need to race-resolve.
pub(crate) fn ensure_typed_array_intrinsic(
) -> (*mut crate::closure::ClosureHeader, *mut ObjectHeader) {
    let existing_ctor = crate::object::TYPED_ARRAY_INTRINSIC_PTR.load(Ordering::Acquire);
    let existing_proto = crate::object::TYPED_ARRAY_INTRINSIC_PROTO_PTR.load(Ordering::Acquire);
    if existing_ctor != 0 && existing_proto != 0 {
        return (
            existing_ctor as *mut crate::closure::ClosureHeader,
            existing_proto as *mut ObjectHeader,
        );
    }
    let ctor = crate::closure::js_closure_alloc(typed_array_constructor_call_thunk as *const u8, 0);
    let proto = js_object_alloc(0, 0);
    if ctor.is_null() || proto.is_null() {
        return (std::ptr::null_mut(), std::ptr::null_mut());
    }
    crate::closure::js_register_closure_arity(typed_array_constructor_call_thunk as *const u8, 0);
    super::super::native_module::set_bound_native_closure_name(ctor, "TypedArray");
    super::super::native_module::set_builtin_closure_length(ctor as usize, 0);
    super::super::set_builtin_property_attrs(
        ctor as usize,
        "name".to_string(),
        super::super::PropertyAttrs::new(false, false, true),
    );
    super::super::set_builtin_property_attrs(
        ctor as usize,
        "length".to_string(),
        super::super::PropertyAttrs::new(false, false, true),
    );
    // Wire `%TypedArray%.prototype` so `getPrototypeOf(Int8Array).prototype`
    // hits a real object instead of undefined.
    let proto_key_bytes = b"prototype";
    let proto_key =
        crate::string::js_string_from_bytes(proto_key_bytes.as_ptr(), proto_key_bytes.len() as u32);
    let proto_value = crate::value::js_nanbox_pointer(proto as i64);
    js_object_set_field_by_name(ctor as *mut ObjectHeader, proto_key, proto_value);
    super::super::set_builtin_property_attrs(
        ctor as usize,
        "prototype".to_string(),
        super::super::PropertyAttrs::new(false, false, false),
    );
    // #2060: the four reflectable `length`/`byteLength`/`byteOffset`/`buffer`
    // accessor descriptors are own properties of `%TypedArray%.prototype` per
    // spec, NOT of the per-kind proto. Pre-#2145 they were installed on each
    // per-kind proto because `getPrototypeOf(per_kind_proto)` returned the
    // per-kind proto itself (identity), so the same lookup happened to land
    // there. After #2145 wires the per-kind protos to share the intrinsic
    // proto, the descriptors must live on the intrinsic itself for
    // `Object.getOwnPropertyDescriptor(getPrototypeOf(Int8Array.prototype),
    // "length")` to keep working.
    install_typed_array_proto_accessors(proto);
    install_typed_array_to_string_tag(proto);
    // The per-kind prototypes (`Int8Array.prototype`, …) inherit ALL of their
    // methods from this shared `%TypedArray%.prototype` (their `[[Prototype]]`),
    // so `Int8Array.prototype.hasOwnProperty("map") === false` and
    // `Int8Array.prototype.map === %TypedArray%.prototype.map` (test262's
    // `prototype/*/inherited.js`).
    //
    // NOTE: the generic `Object.prototype` data methods (`hasOwnProperty`,
    // `valueOf`, …) are intentionally NOT installed here. The intrinsic prototype
    // is allocated with zero inline field slots and already carries ~34 own
    // properties (accessors + `@@iterator` + the spec methods below); adding the
    // extra ~5 crosses an inline-storage boundary that trips a latent field-count
    // overflow (a heap-layout-dependent SIGSEGV under GC pressure). They are not
    // needed for parity — `hasOwnProperty`/`valueOf`/etc. dispatch natively.
    // `toString` is aliased after all constructors are set up in
    // `populate.rs:alias_typed_array_proto_to_string` (ECMA-262 §23.2.3.33).
    // Install the brand-checking spec methods on the shared `%TypedArray%`
    // intrinsic prototype. test262's `testTypedArray.js` harness reads
    // `TypedArray.prototype.<m>` (where `TypedArray ===
    // Object.getPrototypeOf(Int8Array)`), so the brand check for
    // `%TypedArray%.prototype.<m>.call(badReceiver)` must fire when the method
    // is read off the intrinsic, and the per-kind protos resolve their reads
    // here via the `[[Prototype]]` chain.
    typed_array_proto_thunks::install_typed_array_proto_methods(proto);
    // @@iterator must be installed AFTER install_typed_array_proto_methods so
    // that it captures the final `values` closure (the `%TypedArray%.prototype
    // [@@iterator] === %TypedArray%.prototype.values` identity invariant).
    install_typed_array_iterator_symbol(proto);
    install_constructor_static_with_call_arity(
        ctor,
        "from",
        typed_array_from_thunk as *const u8,
        1,
        3,
        false,
    );
    install_constructor_static(ctor, "of", typed_array_of_thunk as *const u8, 0, true);
    crate::object::TYPED_ARRAY_INTRINSIC_PTR.store(ctor as i64, Ordering::Release);
    crate::object::TYPED_ARRAY_INTRINSIC_PROTO_PTR.store(proto as i64, Ordering::Release);
    (ctor, proto)
}

/// Public accessor for the `%TypedArray%.prototype` object. Returns the cached
/// pointer if `populate_global_this_builtins` has run (so the intrinsic is
/// initialised), else null. Used by `js_object_get_prototype_of` to resolve
/// `Object.getPrototypeOf(Int8Array.prototype)` to the shared prototype.
pub(crate) fn typed_array_intrinsic_proto_ptr() -> *mut ObjectHeader {
    crate::object::TYPED_ARRAY_INTRINSIC_PROTO_PTR.load(Ordering::Acquire) as *mut ObjectHeader
}

// ---------------------------------------------------------------------------
// #3664: generator / async-generator intrinsic prototype towers.
// ---------------------------------------------------------------------------
