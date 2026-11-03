//! The `Iterator` global (ECMA-262 §27.1.3): an abstract constructor whose
//! `prototype` is the realm's existing `%IteratorPrototype%` — the object every
//! built-in iterator family already inherits from — never a second object.

use super::iterator_prototypes::{ensure_iterator_prototypes, ITERATOR_PROTOTYPE_PTR};
use super::ObjectHeader;
use crate::closure::{ClosureHeader, JsThis};
use crate::value::JSValue;
use std::sync::atomic::{AtomicU32, Ordering};

const TO_STRING_TAG: &str = "Iterator";

/// How many keys `%Iterator.prototype%` is born with: its intrinsic methods and
/// accessors, none of them a protocol method. A fact of the bootstrap code, the
/// same in every realm, so a plain integer (no heap pointer, not a root).
static BIRTH_KEY_COUNT: AtomicU32 = AtomicU32::new(0);

const PROTOCOL_METHODS: [&[u8]; 3] = [b"next", b"return", b"throw"];

unsafe fn is_protocol_key(key: JSValue) -> bool {
    !key.is_pointer()
        && PROTOCOL_METHODS
            .iter()
            .any(|name| crate::string::js_string_key_matches_bytes(key, name))
}

unsafe fn shape_keys(shape: super::shapes::ShapeRecordRef) -> (*const u64, u32) {
    let keys = shape.keys() as *const crate::array::ArrayHeader;
    (
        crate::array::array_elements_ptr(keys) as *const u64,
        shape.logical_key_count(),
    )
}

/// Record the intrinsic keys installed so far, once they are proven free of
/// protocol methods (a user `return` added before a later install phase must
/// never count as intrinsic).
pub(crate) fn note_birth_keys(proto: *mut ObjectHeader) {
    // SAFETY: `proto` is the live realm `%IteratorPrototype%`.
    unsafe {
        let Some(shape) = super::shapes::object_shape_record(proto) else {
            return;
        };
        let (slots, count) = shape_keys(shape);
        if (0..count).any(|i| is_protocol_key(JSValue::from_bits(*slots.add(i as usize)))) {
            return;
        }
        BIRTH_KEY_COUNT.fetch_max(count, Ordering::Relaxed);
    }
}

/// Does `%Iterator.prototype%`'s shape lack `next`/`return`/`throw`? Keys are
/// appended in insertion order and a delete moves the owner to an overflow
/// (which the step proof rejects first), so only keys past the birth count can
/// be a user addition: the pristine prototype costs one compare.
#[inline]
pub(crate) unsafe fn protocol_free(shape: super::shapes::ShapeRecordRef) -> bool {
    let birth = BIRTH_KEY_COUNT.load(Ordering::Relaxed);
    if shape.logical_key_count() <= birth {
        return true;
    }
    let (slots, count) = shape_keys(shape);
    (birth..count).all(|i| !is_protocol_key(JSValue::from_bits(*slots.add(i as usize))))
}

/// `%Iterator.prototype%`, building the iterator prototype tower on first use.
pub(crate) fn iterator_prototype_object() -> *mut ObjectHeader {
    ensure_iterator_prototypes();
    ITERATOR_PROTOTYPE_PTR.load(Ordering::Acquire) as *mut ObjectHeader
}

/// `Iterator()` and `new Iterator()`: only a subclass's `super()` constructs it.
pub(crate) fn throw_abstract_iterator() -> f64 {
    super::property_ops::throw_type_error("Abstract class Iterator not directly constructable")
}

/// Install the spec accessors on a freshly built `%IteratorPrototype%`, so
/// every tower (any realm, any test fixture) is born with the same keys.
pub(crate) fn install_prototype_accessors(proto: *mut ObjectHeader) {
    install_constructor_accessor(proto);
    install_to_string_tag_accessor(proto);
}

/// `Iterator.prototype.constructor` (§27.1.4.1): an accessor, not a data
/// property. Its getter answers `%Iterator%`, bound by [`bind_constructor`]
/// once the global bootstrap creates it.
fn install_constructor_accessor(proto: *mut ObjectHeader) {
    let get = native_fn(
        crate::fn_info!(iterator_constructor_getter_thunk, 0; with_declared(0)),
        "get constructor",
        0,
        Some(f64::from_bits(crate::value::TAG_UNDEFINED)),
    );
    let set = native_fn(
        crate::fn_info!(iterator_constructor_setter_thunk, 1; with_declared(1)),
        "set constructor",
        1,
        None,
    );
    // SAFETY: `proto` is the live realm `%IteratorPrototype%`.
    unsafe { super::install_builtin_accessor(proto, "constructor", get, set) };
}

/// Point `Iterator.prototype.constructor`'s getter at the realm's `%Iterator%`.
pub(crate) fn bind_constructor(proto: *mut ObjectHeader, ctor_value: f64) {
    let Some(accessor) =
        super::descriptor_state::get_accessor_descriptor(proto as usize, "constructor")
    else {
        return;
    };
    let getter = JSValue::from_bits(accessor.get);
    if getter.is_pointer() {
        crate::closure::js_closure_set_capture_f64(
            getter.as_pointer::<ClosureHeader>() as *mut _,
            0,
            ctor_value,
        );
    }
}

/// `Iterator.prototype[Symbol.toStringTag]` (§27.1.4.14): an accessor too.
fn install_to_string_tag_accessor(proto: *mut ObjectHeader) {
    let sym = crate::symbol::well_known_symbol("toStringTag");
    if proto.is_null() || sym.is_null() {
        return;
    }
    let get = native_fn(
        crate::fn_info!(iterator_to_string_tag_getter_thunk, 0; with_declared(0)),
        "get [Symbol.toStringTag]",
        0,
        None,
    );
    let set = native_fn(
        crate::fn_info!(iterator_to_string_tag_setter_thunk, 1; with_declared(1)),
        "set [Symbol.toStringTag]",
        1,
        None,
    );
    let owner = crate::value::js_nanbox_pointer(proto as i64);
    let key = f64::from_bits(JSValue::pointer(sym as *const u8).bits());
    unsafe {
        crate::symbol::set_symbol_accessor_property(owner, key, get, set);
        crate::symbol::set_symbol_property_attrs(
            proto as usize,
            sym as usize,
            super::PropertyAttrs::new(false, false, true),
        );
    }
}

/// A named native function object; `capture` is stored in its slot 0.
fn native_fn(
    info: *const crate::closure::JsFunctionInfo,
    name: &str,
    length: u32,
    capture: Option<f64>,
) -> u64 {
    let closure = crate::closure::js_closure_alloc(info, capture.is_some() as u32);
    if closure.is_null() {
        return 0;
    }
    if let Some(value) = capture {
        crate::closure::js_closure_set_capture_f64(closure, 0, value);
    }
    super::native_module::set_bound_native_closure_metadata(closure, name, length);
    crate::value::js_nanbox_pointer(closure as i64).to_bits()
}

extern "C" fn iterator_constructor_getter_thunk(
    closure: *const ClosureHeader,
    _this: JsThis,
) -> f64 {
    crate::closure::js_closure_get_capture_f64(closure, 0)
}

extern "C" fn iterator_to_string_tag_getter_thunk(
    _closure: *const ClosureHeader,
    _this: JsThis,
) -> f64 {
    let tag =
        crate::string::js_string_from_bytes(TO_STRING_TAG.as_ptr(), TO_STRING_TAG.len() as u32);
    f64::from_bits(crate::js_nanbox_string(tag as i64).to_bits())
}

extern "C" fn iterator_constructor_setter_thunk(
    _closure: *const ClosureHeader,
    this: JsThis,
    value: f64,
) -> f64 {
    let key = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
    set_ignoring_prototype(
        this,
        f64::from_bits(crate::js_nanbox_string(key as i64).to_bits()),
        value,
    )
}

extern "C" fn iterator_to_string_tag_setter_thunk(
    _closure: *const ClosureHeader,
    this: JsThis,
    value: f64,
) -> f64 {
    let sym = crate::symbol::well_known_symbol("toStringTag");
    let key = f64::from_bits(JSValue::pointer(sym as *const u8).bits());
    set_ignoring_prototype(this, key, value)
}

/// SetterThatIgnoresPrototypeProperties(this, %Iterator.prototype%, key, value)
/// (§27.1.4.1.1): writing through the inherited accessor gives the receiver an
/// own data property and leaves `%Iterator.prototype%` itself untouched.
fn set_ignoring_prototype(this: JsThis, key: f64, value: f64) -> f64 {
    let receiver = this.as_f64();
    let jv = JSValue::from_bits(receiver.to_bits());
    if !jv.is_pointer() {
        return super::property_ops::throw_type_error(
            "Iterator.prototype setter called on non-object",
        );
    }
    if jv.as_pointer::<ObjectHeader>() as *mut ObjectHeader == iterator_prototype_object() {
        return super::property_ops::throw_type_error(
            "Cannot assign to read only property of Iterator.prototype",
        );
    }
    let has_own =
        super::object_ops::js_object_has_own(receiver, key).to_bits() == crate::value::TAG_TRUE;
    if has_own {
        crate::proxy::js_reflect_set(receiver, key, value, receiver);
    } else {
        super::property_ops::create_data_property(receiver, key, value);
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}
