use std::cell::RefCell;

use crate::array::ArrayHeader;
use crate::closure::ClosureHeader;
use crate::typedarray::{
    js_typed_array_get, js_typed_array_set, lookup_typed_array_kind, TypedArrayHeader,
};

thread_local! {
    static TYPED_ARRAY_OWN_PROPS: RefCell<crate::fast_hash::PtrHashMap<usize, Vec<TypedArrayOwnProp>>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
}

#[derive(Clone)]
struct TypedArrayOwnProp {
    key: String,
    value: f64,
    is_data: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TypedArrayStringKeyKind {
    InBoundsIndex(u32),
    IntegerIndex,
    Ordinary,
}

#[derive(Clone, Copy)]
enum TypedArrayOwnerKind {
    TypedArray,
    Uint8ArrayBuffer,
}

fn typed_array_owner_kind(owner: usize) -> Option<TypedArrayOwnerKind> {
    if lookup_typed_array_kind(owner).is_some() {
        Some(TypedArrayOwnerKind::TypedArray)
    } else if crate::buffer::is_uint8array_buffer(owner) {
        Some(TypedArrayOwnerKind::Uint8ArrayBuffer)
    } else {
        None
    }
}

unsafe fn typed_array_owner_length(owner: usize) -> u32 {
    match typed_array_owner_kind(owner) {
        Some(TypedArrayOwnerKind::TypedArray) => (*(owner as *const TypedArrayHeader)).length,
        Some(TypedArrayOwnerKind::Uint8ArrayBuffer) => {
            crate::buffer::js_buffer_length(owner as *const crate::buffer::BufferHeader) as u32
        }
        None => 0,
    }
}

/// `[[ArrayLength]]` of a typed-array / Uint8Array-buffer owner address.
/// Exposed for `TypedArraySpeciesCreate` (the length validation in
/// `TypedArrayCreate`) and the species element-store path.
pub(crate) unsafe fn owner_length(owner: usize) -> u32 {
    typed_array_owner_length(owner)
}

/// Integer-indexed `[[Set]]` used to fill a species-created result. Handles
/// both the `TypedArrayHeader` and Uint8Array-buffer representations and the
/// per-kind `ToNumber`/`ToBigInt` element coercion (a bad BigInt coercion
/// throws). Writes past the result length are silently dropped (a species ctor
/// may return a shorter array; the callback still ran for those indices).
pub(crate) unsafe fn species_result_store(owner: usize, index: usize, raw: f64) {
    if index >= typed_array_owner_length(owner) as usize {
        return;
    }
    match typed_array_owner_kind(owner) {
        Some(TypedArrayOwnerKind::TypedArray) => {
            let ta = owner as *mut TypedArrayHeader;
            let kind = (*ta).kind;
            crate::typedarray::species::store_coerced(ta, index, kind, raw);
        }
        Some(TypedArrayOwnerKind::Uint8ArrayBuffer) => {
            let n = crate::typedarray::species::to_number(raw);
            crate::buffer::js_buffer_set(
                owner as *mut crate::buffer::BufferHeader,
                index as i32,
                n as i32,
            );
        }
        None => {}
    }
}

unsafe fn typed_array_owner_get(owner: usize, index: u32) -> f64 {
    match typed_array_owner_kind(owner) {
        Some(TypedArrayOwnerKind::TypedArray) => {
            js_typed_array_get(owner as *const TypedArrayHeader, index as i32)
        }
        Some(TypedArrayOwnerKind::Uint8ArrayBuffer) => {
            crate::buffer::js_buffer_get(owner as *const crate::buffer::BufferHeader, index as i32)
                as f64
        }
        None => f64::from_bits(crate::value::TAG_UNDEFINED),
    }
}

unsafe fn typed_array_owner_set(owner: usize, index: u32, value: f64) {
    match typed_array_owner_kind(owner) {
        Some(TypedArrayOwnerKind::TypedArray) => {
            js_typed_array_set(owner as *mut TypedArrayHeader, index as i32, value);
        }
        Some(TypedArrayOwnerKind::Uint8ArrayBuffer) => {
            crate::buffer::js_buffer_set(
                owner as *mut crate::buffer::BufferHeader,
                index as i32,
                value as i32,
            );
        }
        None => {}
    }
}

pub(crate) fn typed_array_clear_own_props(owner: usize) {
    TYPED_ARRAY_OWN_PROPS.with(|m| {
        m.borrow_mut().remove(&owner);
    });
}

pub(crate) fn typed_array_addr_from_value(value: f64) -> Option<usize> {
    let jsval = crate::value::JSValue::from_bits(value.to_bits());
    let valid_addr = |addr: usize| {
        (addr > 0x10000 && addr <= crate::value::POINTER_MASK as usize && addr & 0x7 == 0)
            .then_some(addr)
            .filter(|addr| typed_array_owner_kind(*addr).is_some())
    };
    if jsval.is_pointer() {
        return valid_addr(jsval.as_pointer::<u8>() as usize);
    }
    let bits = value.to_bits();
    if let Some(addr) = valid_addr(bits as usize) {
        return Some(addr);
    }
    if value.is_finite() && value.fract() == 0.0 && value > 0.0 {
        return valid_addr(value as usize);
    }
    None
}

unsafe fn string_header_str<'a>(key: *const crate::string::StringHeader) -> Option<&'a str> {
    if key.is_null() || (key as usize) < 0x10000 {
        return None;
    }
    let len = (*key).byte_len as usize;
    let data = (key as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
    std::str::from_utf8(std::slice::from_raw_parts(data, len)).ok()
}

fn unsigned_canonical_index(name: &str) -> Option<u32> {
    if name == "0" {
        return Some(0);
    }
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes[0] == b'0' || !bytes.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let idx = name.parse::<u32>().ok()?;
    if idx.to_string() == name {
        Some(idx)
    } else {
        None
    }
}

fn is_canonical_numeric_index_name(name: &str) -> bool {
    if matches!(name, "-0" | "NaN" | "Infinity" | "-Infinity") {
        return true;
    }
    let Ok(value) = name.parse::<f64>() else {
        return false;
    };
    if !value.is_finite() {
        return false;
    }
    // CanonicalNumericIndexString requires `ToString(ToNumber(name)) == name`
    // with the JS Number→String rendering — Rust's `format!` prints `1e21` as
    // `1000000000000000000000` and `1e-7` as `0.0000001`, which wrongly
    // classified those keys as canonical (JS renders `1e+21` / `1e-7`, so
    // they are ORDINARY keys).
    let rendered = crate::string::js_number_to_string(value);
    if rendered.is_null() {
        return false;
    }
    unsafe { string_header_str(rendered as *const crate::string::StringHeader) == Some(name) }
}

fn typed_array_string_key_kind(name: &str, len: u32) -> TypedArrayStringKeyKind {
    if let Some(index) = unsigned_canonical_index(name) {
        if index < len && index <= i32::MAX as u32 {
            TypedArrayStringKeyKind::InBoundsIndex(index)
        } else {
            TypedArrayStringKeyKind::IntegerIndex
        }
    } else if is_canonical_numeric_index_name(name) {
        TypedArrayStringKeyKind::IntegerIndex
    } else {
        TypedArrayStringKeyKind::Ordinary
    }
}

fn typed_array_value(ta: *const TypedArrayHeader) -> f64 {
    crate::value::js_nanbox_pointer(ta as i64)
}

fn invoke_typed_array_accessor_getter(get_bits: u64, receiver: f64) -> f64 {
    let closure = (get_bits & crate::value::POINTER_MASK) as *const ClosureHeader;
    if closure.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let prev = crate::object::js_implicit_this_set(receiver);
    let result = crate::closure::js_closure_call0(closure);
    crate::object::js_implicit_this_set(prev);
    result
}

fn invoke_typed_array_accessor_setter(set_bits: u64, receiver: f64, value: f64) {
    let closure = (set_bits & crate::value::POINTER_MASK) as *const ClosureHeader;
    if closure.is_null() {
        return;
    }
    let prev = crate::object::js_implicit_this_set(receiver);
    crate::closure::js_closure_call1(closure, value);
    crate::object::js_implicit_this_set(prev);
}

fn barrier_typed_array_own_props(owner: usize, props: &mut [TypedArrayOwnProp]) {
    for prop in props.iter_mut().filter(|prop| prop.is_data) {
        crate::gc::runtime_write_barrier_external_slot(
            owner,
            &mut prop.value as *mut f64 as usize,
            prop.value.to_bits(),
        );
    }
}

fn upsert_typed_array_own_prop(owner: usize, key: String, value: f64, is_data: bool) {
    TYPED_ARRAY_OWN_PROPS.with(|m| {
        let mut map = m.borrow_mut();
        let props = map.entry(owner).or_default();
        if let Some(prop) = props.iter_mut().find(|prop| prop.key == key) {
            prop.value = value;
            prop.is_data = is_data;
        } else {
            props.push(TypedArrayOwnProp {
                key,
                value,
                is_data,
            });
        }
        barrier_typed_array_own_props(owner, props);
    });
}

fn remove_typed_array_own_prop(owner: usize, key: &str) -> bool {
    TYPED_ARRAY_OWN_PROPS.with(|m| {
        let mut map = m.borrow_mut();
        let Some(props) = map.get_mut(&owner) else {
            return false;
        };
        let Some(index) = props.iter().position(|prop| prop.key == key) else {
            return false;
        };
        props.remove(index);
        if props.is_empty() {
            map.remove(&owner);
        }
        true
    })
}

fn typed_array_own_prop_snapshot(owner: usize, key: &str) -> Option<TypedArrayOwnProp> {
    TYPED_ARRAY_OWN_PROPS.with(|m| {
        m.borrow()
            .get(&owner)
            .and_then(|props| props.iter().find(|prop| prop.key == key).cloned())
    })
}

fn typed_array_has_ordinary_own_prop(owner: usize, key: &str) -> bool {
    TYPED_ARRAY_OWN_PROPS.with(|m| {
        m.borrow()
            .get(&owner)
            .is_some_and(|props| props.iter().any(|prop| prop.key == key))
    })
}

unsafe fn descriptor_has(desc_ptr: *mut crate::object::ObjectHeader, name: &[u8]) -> bool {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::object::own_key_present(desc_ptr, key)
}

unsafe fn descriptor_read(
    desc_ptr: *mut crate::object::ObjectHeader,
    name: &[u8],
) -> crate::JSValue {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::object::js_object_get_field_by_name(desc_ptr as *const crate::object::ObjectHeader, key)
}

unsafe fn descriptor_bool(desc_ptr: *mut crate::object::ObjectHeader, name: &[u8]) -> Option<bool> {
    if !descriptor_has(desc_ptr, name) {
        return None;
    }
    let value = descriptor_read(desc_ptr, name);
    Some(crate::value::js_is_truthy(f64::from_bits(value.bits())) != 0)
}

fn throw_typed_array_define_error(message: String) -> ! {
    throw_type_error(message.as_bytes())
}

thread_local! {
    /// Typed arrays marked non-extensible by `Object.preventExtensions`.
    /// A SIDE TABLE, not the GC-header flag: small typed arrays are plain
    /// `alloc`ed without a `GcHeader`, so flag reads/writes at `addr - 8`
    /// would touch allocator metadata (observed as random `NO_EXTEND` reads
    /// and heap corruption).
    static TYPED_ARRAY_NO_EXTEND: RefCell<std::collections::HashSet<usize>> =
        RefCell::new(std::collections::HashSet::new());
}

/// Mark a typed array non-extensible (`Object.preventExtensions(ta)`).
pub(crate) fn typed_array_mark_no_extend(owner: usize) {
    TYPED_ARRAY_NO_EXTEND.with(|s| {
        s.borrow_mut().insert(owner);
    });
}

/// Has `Object.preventExtensions(ta)` run for this typed array?
pub(crate) fn typed_array_owner_no_extend(owner: usize) -> bool {
    TYPED_ARRAY_NO_EXTEND.with(|s| s.borrow().contains(&owner))
}

/// Drop the non-extensible mark when a typed array is collected (called from
/// `unregister_typed_array`, mirroring the own-props cleanup).
pub(crate) fn typed_array_clear_no_extend(owner: usize) {
    TYPED_ARRAY_NO_EXTEND.with(|s| {
        s.borrow_mut().remove(&owner);
    });
}

#[cold]
fn throw_type_error(message: &[u8]) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

pub(crate) unsafe fn typed_array_define_own_property(
    obj_value: f64,
    ta: *mut TypedArrayHeader,
    key: *const crate::string::StringHeader,
    key_name: &str,
    descriptor_value: f64,
) -> f64 {
    if ta.is_null() {
        return obj_value;
    }
    let owner = ta as usize;
    let len = typed_array_owner_length(owner);
    let desc_ptr = crate::object::extract_obj_ptr(descriptor_value);
    if desc_ptr.is_null() {
        return obj_value;
    }
    match typed_array_string_key_kind(key_name, len) {
        TypedArrayStringKeyKind::InBoundsIndex(index) => {
            let has_accessor = descriptor_has(desc_ptr, b"get") || descriptor_has(desc_ptr, b"set");
            let writable = descriptor_bool(desc_ptr, b"writable");
            let enumerable = descriptor_bool(desc_ptr, b"enumerable");
            let configurable = descriptor_bool(desc_ptr, b"configurable");
            if has_accessor
                || writable.is_some_and(|value| !value)
                || enumerable.is_some_and(|value| !value)
                || configurable.is_some_and(|value| !value)
            {
                throw_typed_array_define_error(format!("Cannot redefine property: {key_name}"));
            }
            if descriptor_has(desc_ptr, b"value") {
                let value = descriptor_read(desc_ptr, b"value");
                typed_array_owner_set(owner, index, f64::from_bits(value.bits()));
            }
            obj_value
        }
        TypedArrayStringKeyKind::IntegerIndex => {
            throw_type_error(b"Invalid typed array index");
        }
        TypedArrayStringKeyKind::Ordinary => {
            // OrdinaryDefineOwnProperty step 2: a brand-new key on a
            // non-extensible typed array is rejected (`Object.defineProperty`
            // throws; the `Reflect` path pre-checks extensibility itself and
            // returns false before reaching here).
            if !typed_array_has_ordinary_own_prop(owner, key_name)
                && typed_array_owner_no_extend(owner)
            {
                throw_typed_array_define_error(format!(
                    "Cannot define property {key_name}, object is not extensible"
                ));
            }
            let has_get = descriptor_has(desc_ptr, b"get");
            let has_set = descriptor_has(desc_ptr, b"set");
            let has_accessor = has_get || has_set;
            if has_accessor {
                let get_field = descriptor_read(desc_ptr, b"get");
                let set_field = descriptor_read(desc_ptr, b"set");
                let get_bits = if !has_get || get_field.is_undefined() {
                    0
                } else {
                    crate::closure::clone_closure_rebind_this(get_field.bits(), obj_value)
                };
                let set_bits = if !has_set || set_field.is_undefined() {
                    0
                } else {
                    crate::closure::clone_closure_rebind_this(set_field.bits(), obj_value)
                };
                crate::object::set_accessor_descriptor(
                    owner,
                    key_name.to_string(),
                    crate::object::AccessorDescriptor {
                        get: get_bits,
                        set: set_bits,
                    },
                );
                upsert_typed_array_own_prop(
                    owner,
                    key_name.to_string(),
                    f64::from_bits(crate::value::TAG_UNDEFINED),
                    false,
                );
            } else {
                crate::object::clear_accessor_descriptor(owner, key_name);
                let value = if descriptor_has(desc_ptr, b"value") {
                    let value = descriptor_read(desc_ptr, b"value");
                    f64::from_bits(value.bits())
                } else {
                    f64::from_bits(crate::value::TAG_UNDEFINED)
                };
                upsert_typed_array_own_prop(owner, key_name.to_string(), value, true);
            }
            let writable = descriptor_bool(desc_ptr, b"writable").unwrap_or(has_accessor);
            let enumerable = descriptor_bool(desc_ptr, b"enumerable").unwrap_or(false);
            let configurable = descriptor_bool(desc_ptr, b"configurable").unwrap_or(false);
            crate::object::set_property_attrs(
                owner,
                key_name.to_string(),
                crate::object::PropertyAttrs::new(writable, enumerable, configurable),
            );
            let _ = key;
            obj_value
        }
    }
}

pub(crate) unsafe fn typed_array_set_own_property(
    ta: *mut TypedArrayHeader,
    key: *const crate::string::StringHeader,
    value: f64,
) -> bool {
    if ta.is_null() || key.is_null() {
        return false;
    }
    let Some(name) = string_header_str(key) else {
        return false;
    };
    let owner = ta as usize;
    typed_array_set_property_by_name(owner, name, value)
}

/// Ordinary `[[Set]]` (§10.4.5.5 step 2 falling to OrdinarySet) of a
/// string-keyed property on a typed array. Returns the spec `[[Set]]`
/// boolean: `true` when the write is accepted (or spec-silently-ignored, e.g.
/// a canonical out-of-bounds index), `false` when it is REJECTED — a
/// non-writable own data property, a setter-less accessor, or a new key on a
/// non-extensible typed array. A `false` return is what makes
/// `Reflect.set(ta, k, v)` report `false` and a strict `ta.k = v` throw a
/// TypeError, matching a plain object's OrdinaryDefineOwnProperty rejection
/// (previously every path returned `true`, silently swallowing the rejection —
/// test262 TypedArrayConstructors/internals/Set key-is-not-numeric-index).
pub(crate) unsafe fn typed_array_set_property_by_name(
    owner: usize,
    name: &str,
    value: f64,
) -> bool {
    if typed_array_owner_kind(owner).is_none() {
        return false;
    }
    match typed_array_string_key_kind(name, typed_array_owner_length(owner)) {
        TypedArrayStringKeyKind::InBoundsIndex(index) => {
            typed_array_owner_set(owner, index, value);
            true
        }
        TypedArrayStringKeyKind::IntegerIndex => {
            // TypedArraySetElement (§10.4.5.16) coerces the value BEFORE the
            // IsValidIntegerIndex bounds check: a canonical-but-invalid index
            // (out of bounds, `-0`, `1.5`, `NaN`, …) still runs `ToNumber` /
            // `ToBigInt` on the value for its observable side effects (a
            // `valueOf`/`Symbol.toPrimitive` hook, or a throw), then silently
            // drops the store. Skipping the coercion made `ta[100] = {valueOf}`
            // never fire the hook (test262 Set/tonumber-value-throws,
            // key-is-out-of-bounds-receiver-is-proto).
            typed_array_coerce_element_for_side_effects(owner, value);
            true
        }
        TypedArrayStringKeyKind::Ordinary => {
            if let Some(acc) = crate::object::get_accessor_descriptor(owner, name) {
                if acc.set != 0 {
                    invoke_typed_array_accessor_setter(
                        acc.set,
                        typed_array_value(owner as *const TypedArrayHeader),
                        value,
                    );
                    return true;
                }
                // Accessor own property with no setter: OrdinarySet rejects.
                return false;
            }
            if typed_array_has_ordinary_own_prop(owner, name) {
                if let Some(attrs) = crate::object::get_property_attrs(owner, name) {
                    if !attrs.writable() {
                        // Existing non-writable own data property: reject.
                        return false;
                    }
                }
            } else {
                // A brand-new own property on a non-extensible typed array
                // cannot be created (OrdinaryDefineOwnProperty step: extensible
                // is false → return false). An existing key can still be
                // updated (handled by the writable branch above).
                if typed_array_owner_no_extend(owner) {
                    return false;
                }
                crate::object::set_property_attrs(
                    owner,
                    name.to_string(),
                    crate::object::PropertyAttrs::new(true, true, true),
                );
            }
            upsert_typed_array_own_prop(owner, name.to_string(), value, true);
            true
        }
    }
}

/// Run the per-content-type element coercion (`ToNumber` for numeric views,
/// `ToBigInt` for BigInt views) purely for its observable side effects —
/// invoking a value's `valueOf`/`Symbol.toPrimitive` and propagating any abrupt
/// completion. Used on the canonical-invalid-index write path where the store
/// itself is dropped but the coercion must still happen (TypedArraySetElement
/// step 1 runs before the IsValidIntegerIndex check).
unsafe fn typed_array_coerce_element_for_side_effects(owner: usize, value: f64) {
    match typed_array_owner_kind(owner) {
        Some(TypedArrayOwnerKind::TypedArray) => {
            let kind = (*(owner as *const TypedArrayHeader)).kind;
            // `coerce_for_kind` performs ToBigInt for BigInt views (throwing on
            // a Number) and ToNumber otherwise; its result is discarded — only
            // the side effect / throw matters.
            let _ = crate::typedarray::bigint::coerce_for_kind(kind, value);
        }
        Some(TypedArrayOwnerKind::Uint8ArrayBuffer) => {
            let _ = crate::typedarray::species::to_number(value);
        }
        None => {}
    }
}

pub(crate) unsafe fn typed_array_set_numeric_index(owner: usize, index: f64, value: f64) -> bool {
    if typed_array_owner_kind(owner).is_none() {
        return false;
    }
    if !index.is_finite() || index.fract() != 0.0 || index < 0.0 || index > u32::MAX as f64 {
        // Canonical-invalid index: coerce the value for side effects, then drop.
        typed_array_coerce_element_for_side_effects(owner, value);
        return true;
    }
    let index = index as u32;
    if index < typed_array_owner_length(owner) {
        typed_array_owner_set(owner, index, value);
    } else {
        // In-range integer but past [[ArrayLength]] (out of bounds): still
        // coerce for side effects before dropping the store.
        typed_array_coerce_element_for_side_effects(owner, value);
    }
    true
}

pub(crate) unsafe fn typed_array_get_own_property_value(
    ta: *const TypedArrayHeader,
    key: *const crate::string::StringHeader,
) -> Option<f64> {
    if ta.is_null() || key.is_null() {
        return None;
    }
    let name = string_header_str(key)?;
    let owner = ta as usize;
    typed_array_get_property_value_by_name(owner, name)
}

pub(crate) unsafe fn typed_array_get_property_value_by_name(
    owner: usize,
    name: &str,
) -> Option<f64> {
    typed_array_owner_kind(owner)?;
    match typed_array_string_key_kind(name, typed_array_owner_length(owner)) {
        TypedArrayStringKeyKind::InBoundsIndex(index) => Some(typed_array_owner_get(owner, index)),
        TypedArrayStringKeyKind::IntegerIndex => Some(f64::from_bits(crate::value::TAG_UNDEFINED)),
        TypedArrayStringKeyKind::Ordinary => {
            let prop = typed_array_own_prop_snapshot(owner, name)?;
            if prop.is_data {
                return Some(prop.value);
            }
            let Some(acc) = crate::object::get_accessor_descriptor(owner, name) else {
                return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
            };
            if acc.get == 0 {
                Some(f64::from_bits(crate::value::TAG_UNDEFINED))
            } else {
                Some(invoke_typed_array_accessor_getter(
                    acc.get,
                    typed_array_value(owner as *const TypedArrayHeader),
                ))
            }
        }
    }
}

pub(crate) unsafe fn typed_array_get_numeric_index(owner: usize, index: f64) -> Option<f64> {
    typed_array_owner_kind(owner)?;
    if !index.is_finite() || index.fract() != 0.0 || index < 0.0 || index > u32::MAX as f64 {
        return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
    }
    let index = index as u32;
    if index < typed_array_owner_length(owner) {
        Some(typed_array_owner_get(owner, index))
    } else {
        Some(f64::from_bits(crate::value::TAG_UNDEFINED))
    }
}

pub(crate) unsafe fn typed_array_index_get_dynamic(owner_bits: usize, key: f64) -> f64 {
    let Some(owner) = typed_array_addr_from_value(f64::from_bits(owner_bits as u64)) else {
        // #5989: a `u8.subarray(...)` / `u8.slice(...)` of a BufferHeader-backed
        // Uint8Array returns another (uint8array-marked) BUFFER, which the
        // typed-array registry gate above doesn't know — a statically-typed
        // `r[i]` on such a value silently read `undefined` (react-server-dom's
        // flight row parser walks exactly these chunk views). Route buffer
        // receivers through the generic dynamic index path, which handles
        // BufferHeader indexing (numeric, string, and symbol keys) correctly.
        let addr = owner_bits & crate::value::POINTER_MASK as usize;
        if addr != 0 && crate::buffer::is_registered_buffer(addr) {
            return crate::value::js_dyn_index_get(
                crate::value::js_nanbox_pointer(addr as i64),
                key,
            );
        }
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    // A Symbol key is never an integer-indexed element — read it from the symbol
    // side table, exactly as the ordinary `obj[sym]` path does. Without this a
    // symbol coerces to a numeric index (NaN → undefined) and a previously
    // stored `ta[sym]` reads back `undefined` (test262 TypedArray symbol-key
    // internals, #5735).
    if crate::symbol::js_is_symbol(key) != 0 {
        return crate::symbol::js_object_get_symbol_property(
            crate::value::js_nanbox_pointer(owner as i64),
            key,
        );
    }
    let jsval = crate::value::JSValue::from_bits(key.to_bits());
    if jsval.is_string() || jsval.is_short_string() {
        let key_ptr =
            crate::value::js_get_string_pointer_unified(key) as *const crate::string::StringHeader;
        if key_ptr.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        if let Some(value) =
            typed_array_get_own_property_value(owner as *const TypedArrayHeader, key_ptr)
        {
            return value;
        }
        return crate::object::js_object_get_field_by_name_f64(
            owner as *const crate::object::ObjectHeader,
            key_ptr,
        );
    }
    if jsval.is_int32() {
        return typed_array_get_numeric_index(owner, jsval.as_int32() as f64)
            .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
    }
    if key.is_finite() {
        return typed_array_get_numeric_index(owner, key)
            .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_typed_array_index_set_dynamic(
    ta: *mut TypedArrayHeader,
    key: f64,
    value: f64,
) -> f64 {
    unsafe {
        let Some(owner) = typed_array_addr_from_value(f64::from_bits(ta as u64)) else {
            // #5989: BufferHeader-backed receivers (subarray/slice results) —
            // mirror the get-side fallback so statically-typed `r[i] = v`
            // stores aren't silently dropped.
            let addr = (ta as usize) & crate::value::POINTER_MASK as usize;
            if addr != 0 && crate::buffer::is_registered_buffer(addr) {
                return crate::value::js_dyn_index_set(
                    crate::value::js_nanbox_pointer(addr as i64),
                    key,
                    value,
                );
            }
            return value;
        };
        // A Symbol key is never an integer-indexed element — store it in the
        // symbol side table, exactly as the ordinary `obj[sym] = v` path does.
        // Without this the symbol is silently dropped (and, on the codegen
        // width-tracked store path, `fptosi` coerces it to index 0 and clobbers
        // element 0) — test262 TypedArray symbol-key internals, #5735.
        if crate::symbol::js_is_symbol(key) != 0 {
            crate::symbol::js_object_set_symbol_property(
                crate::value::js_nanbox_pointer(owner as i64),
                key,
                value,
            );
            return value;
        }
        let jsval = crate::value::JSValue::from_bits(key.to_bits());
        if jsval.is_string() || jsval.is_short_string() {
            let key_ptr = crate::value::js_get_string_pointer_unified(key)
                as *const crate::string::StringHeader;
            if let Some(name) = string_header_str(key_ptr) {
                typed_array_set_property_by_name(owner, name, value);
            }
            return value;
        }
        if jsval.is_int32() {
            typed_array_set_numeric_index(owner, jsval.as_int32() as f64, value);
        } else if key.is_finite() {
            typed_array_set_numeric_index(owner, key, value);
        }
        value
    }
}

#[used]
static KEEP_JS_TYPED_ARRAY_INDEX_SET_DYNAMIC: extern "C" fn(
    *mut TypedArrayHeader,
    f64,
    f64,
) -> f64 = js_typed_array_index_set_dynamic;

pub(crate) unsafe fn typed_array_has_own_property(
    ta: *const TypedArrayHeader,
    key: *const crate::string::StringHeader,
) -> bool {
    if ta.is_null() || key.is_null() {
        return false;
    }
    let Some(name) = string_header_str(key) else {
        return false;
    };
    let owner = ta as usize;
    match typed_array_string_key_kind(name, typed_array_owner_length(owner)) {
        TypedArrayStringKeyKind::InBoundsIndex(_) => true,
        TypedArrayStringKeyKind::IntegerIndex => false,
        TypedArrayStringKeyKind::Ordinary => typed_array_has_ordinary_own_prop(owner, name),
    }
}

/// Full `[[HasProperty]]` for a TypedArray (`key in ta`): a canonical numeric
/// index resolves by bounds only (never the prototype chain), while an
/// ordinary key falls back to OrdinaryHasProperty — own expandos, then the
/// `[[Prototype]]` chain (`%TypedArray%.prototype` methods/accessors, the
/// per-kind prototype, then `Object.prototype`).
pub(crate) unsafe fn typed_array_has_property(
    ta: *const TypedArrayHeader,
    key: *const crate::string::StringHeader,
) -> bool {
    if ta.is_null() || key.is_null() {
        return false;
    }
    let Some(name) = string_header_str(key) else {
        return false;
    };
    let owner = ta as usize;
    match typed_array_string_key_kind(name, typed_array_owner_length(owner)) {
        TypedArrayStringKeyKind::InBoundsIndex(_) => true,
        TypedArrayStringKeyKind::IntegerIndex => false,
        TypedArrayStringKeyKind::Ordinary => {
            typed_array_has_ordinary_own_prop(owner, name)
                || typed_array_prototype_chain_has(owner, name)
        }
    }
}

/// Would an ordinary string key resolve somewhere on a typed array's
/// `[[Prototype]]` chain? Checks the shared `%TypedArray%.prototype` intrinsic
/// object (spec methods + the reflectable accessors), the per-kind prototype
/// object (`Float64Array.prototype` — `constructor` and any user patches),
/// and finally `Object.prototype` (its universal methods + user expandos).
pub(crate) unsafe fn typed_array_prototype_chain_has(owner: usize, name: &str) -> bool {
    // Build the shared `%TypedArray%.prototype` intrinsic if it hasn't been yet,
    // so the membership check doesn't depend on a registered typed array having
    // been created first (#6164 — otherwise the result was creation-order
    // dependent, and buffer-backed `Uint8Array` never populated it).
    let _ = crate::object::ensure_typed_array_intrinsic();
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    // %TypedArray%.prototype intrinsic.
    let intrinsic = crate::object::typed_array_intrinsic_proto_ptr();
    if !intrinsic.is_null() {
        if crate::object::own_key_present(intrinsic, key) {
            return true;
        }
        if crate::object::get_accessor_descriptor(intrinsic as usize, name).is_some() {
            return true;
        }
    }
    // Per-kind prototype object (constructor, user patches).
    if name == "constructor" {
        return true;
    }
    if let Some(kind) = typed_array_owner_kind_id(owner) {
        let ctor_name = crate::typedarray::name_for_kind(kind);
        let ctor =
            crate::object::js_get_global_this_builtin_value(ctor_name.as_ptr(), ctor_name.len());
        let raw = crate::value::js_nanbox_get_pointer(ctor) as usize;
        if raw >= 0x10000 {
            let proto = crate::closure::closure_get_dynamic_prop(raw, "prototype");
            let proto_raw = crate::value::js_nanbox_get_pointer(proto) as usize;
            if proto_raw >= 0x10000 {
                if crate::object::own_key_present(
                    proto_raw as *mut crate::object::ObjectHeader,
                    key,
                ) {
                    return true;
                }
                if crate::object::get_accessor_descriptor(proto_raw, name).is_some() {
                    return true;
                }
            }
        }
    }
    // Object.prototype: universal methods plus user expandos.
    if matches!(
        name,
        "toString"
            | "toLocaleString"
            | "valueOf"
            | "hasOwnProperty"
            | "isPrototypeOf"
            | "propertyIsEnumerable"
            | "__proto__"
    ) {
        return true;
    }
    let obj_proto = crate::object::builtin_prototype_value("Object");
    let obj_proto_raw = crate::value::js_nanbox_get_pointer(obj_proto) as usize;
    if obj_proto_raw >= 0x10000 {
        if crate::object::own_key_present(obj_proto_raw as *mut crate::object::ObjectHeader, key) {
            return true;
        }
        if crate::object::get_accessor_descriptor(obj_proto_raw, name).is_some() {
            return true;
        }
    }
    false
}

/// The element kind for a TypedArray owner address (`None` for the
/// `BufferHeader`-backed `Uint8Array` representation).
fn typed_array_owner_kind_id(owner: usize) -> Option<u8> {
    lookup_typed_array_kind(owner)
}

/// Classify a string key against a typed array's CanonicalNumericIndexString
/// rule: `Some(true)` = valid in-bounds integer index, `Some(false)` =
/// canonical numeric index that is NOT a valid index (out of bounds, `-1`,
/// `1.5`, `-0`, …), `None` = ordinary key. Used by the exotic `[[Set]]`
/// interception (a canonical index never consults the prototype chain).
pub(crate) fn typed_array_canonical_index_validity(owner: usize, name: &str) -> Option<bool> {
    let len = unsafe { typed_array_owner_length(owner) };
    match typed_array_string_key_kind(name, len) {
        TypedArrayStringKeyKind::InBoundsIndex(_) => Some(true),
        TypedArrayStringKeyKind::IntegerIndex => Some(false),
        TypedArrayStringKeyKind::Ordinary => None,
    }
}

/// The kind of own set-descriptor an ordinary (non-index) string key carries on
/// a typed array, as seen by OrdinarySet. Typed arrays keep their expando own
/// properties in the side tables (`TYPED_ARRAY_OWN_PROPS` +
/// `PROPERTY_DESCRIPTORS`/`ACCESSOR_DESCRIPTORS`), which the generic
/// `own_set_descriptor` walk skips because `object_has_descriptors` is gated off
/// for typed arrays. This exposes that state to the receiver-threading `[[Set]]`
/// so `Reflect.set(ta, k, v)` reports the right boolean.
#[derive(Clone, Copy)]
pub(crate) enum TypedArrayOwnSetDescriptor {
    /// Data property present with the given writability.
    Data { writable: bool },
    /// Accessor property; `setter_bits` == 0 means setter-less (rejects).
    Accessor { setter_bits: u64 },
}

/// Probe the own set-descriptor for an ordinary string key on a typed array.
/// Returns `None` when the key names no own expando property (the caller then
/// falls to CreateDataProperty on the receiver). Index/canonical-index keys are
/// never ordinary properties, so they also return `None` here.
pub(crate) fn typed_array_own_set_descriptor(
    owner: usize,
    name: &str,
) -> Option<TypedArrayOwnSetDescriptor> {
    if typed_array_canonical_index_validity(owner, name).is_some() {
        // Index / canonical-index keys are never ordinary expando properties.
        return None;
    }
    if let Some(acc) = crate::object::get_accessor_descriptor(owner, name) {
        return Some(TypedArrayOwnSetDescriptor::Accessor {
            setter_bits: acc.set,
        });
    }
    if typed_array_has_ordinary_own_prop(owner, name) {
        let writable = crate::object::get_property_attrs(owner, name)
            .map(|attrs| attrs.writable())
            .unwrap_or(true);
        return Some(TypedArrayOwnSetDescriptor::Data { writable });
    }
    None
}

/// `OrdinaryToPrimitive(O, number)` own-expando probe for a typed array used
/// as a *coercion source*: a patched own `valueOf`/`toString` (stored in the
/// typed-array own-props side table, invisible to the generic object helpers)
/// runs with `this` = the view, propagating abrupt completions. Returns
/// `Some(primitive)` when a patched method produced a non-object; `None` when
/// no own patch applies (caller falls back to its default coercion).
pub(crate) unsafe fn typed_array_own_to_primitive_number(owner: usize, value: f64) -> Option<f64> {
    for name in ["valueOf", "toString"] {
        let Some(m) = typed_array_get_property_value_by_name(owner, name) else {
            continue;
        };
        let mbits = m.to_bits();
        if (mbits >> 48) != 0x7FFD
            || !crate::closure::is_closure_ptr((mbits & crate::value::POINTER_MASK) as usize)
        {
            continue;
        }
        let bound = crate::closure::clone_closure_rebind_this(mbits, value);
        let r = crate::closure::js_native_call_value(f64::from_bits(bound), std::ptr::null(), 0);
        let rb = r.to_bits();
        let is_object = (rb >> 48) == 0x7FFD
            && crate::symbol::js_is_symbol(r) == 0
            && (rb & crate::value::POINTER_MASK) >= 0x10000;
        if !is_object {
            return Some(r);
        }
    }
    None
}

pub(crate) unsafe fn typed_array_property_is_enumerable(
    ta: *const TypedArrayHeader,
    key: *const crate::string::StringHeader,
) -> bool {
    if ta.is_null() || key.is_null() {
        return false;
    }
    let Some(name) = string_header_str(key) else {
        return false;
    };
    let owner = ta as usize;
    match typed_array_string_key_kind(name, typed_array_owner_length(owner)) {
        TypedArrayStringKeyKind::InBoundsIndex(_) => true,
        TypedArrayStringKeyKind::IntegerIndex => false,
        TypedArrayStringKeyKind::Ordinary => {
            if !typed_array_has_ordinary_own_prop(owner, name) {
                return false;
            }
            crate::object::get_property_attrs(owner, name)
                .map(|attrs| attrs.enumerable())
                .unwrap_or(true)
        }
    }
}

fn typed_array_non_index_keys(owner: usize, enumerable_only: bool) -> Vec<String> {
    let mut keys = TYPED_ARRAY_OWN_PROPS.with(|m| {
        m.borrow()
            .get(&owner)
            .map(|props| {
                props
                    .iter()
                    .filter_map(|prop| {
                        if enumerable_only {
                            let enumerable = crate::object::get_property_attrs(owner, &prop.key)
                                .map(|attrs| attrs.enumerable())
                                .unwrap_or(true);
                            if !enumerable {
                                return None;
                            }
                        }
                        Some(prop.key.clone())
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });
    for key in crate::object::accessor_descriptor_keys_for_obj(owner) {
        if keys.iter().any(|existing| existing == &key) {
            continue;
        }
        if enumerable_only {
            let enumerable = crate::object::get_property_attrs(owner, &key)
                .map(|attrs| attrs.enumerable())
                .unwrap_or(false);
            if !enumerable {
                continue;
            }
        }
        keys.push(key);
    }
    keys
}

pub(crate) unsafe fn typed_array_own_property_names(
    ta: *const TypedArrayHeader,
    enumerable_only: bool,
) -> *mut ArrayHeader {
    if ta.is_null() {
        return crate::array::js_array_alloc(0);
    }
    let owner = ta as usize;
    let len = typed_array_owner_length(owner);
    let names = typed_array_non_index_keys(owner, enumerable_only);
    let mut result = crate::array::js_array_alloc(len.saturating_add(names.len() as u32));
    for i in 0..len {
        let name = i.to_string();
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        result = crate::array::js_array_push(result, crate::JSValue::string_ptr(key));
    }
    for name in names {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        result = crate::array::js_array_push(result, crate::JSValue::string_ptr(key));
    }
    result
}

pub(crate) unsafe fn typed_array_own_enumerable_values(
    ta: *const TypedArrayHeader,
) -> *mut ArrayHeader {
    if ta.is_null() {
        return crate::array::js_array_alloc(0);
    }
    let owner = ta as usize;
    let len = typed_array_owner_length(owner);
    let names = typed_array_non_index_keys(owner, true);
    let mut result = crate::array::js_array_alloc(len.saturating_add(names.len() as u32));
    for i in 0..len {
        result = crate::array::js_array_push_f64(result, typed_array_owner_get(owner, i));
    }
    for name in names {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        if let Some(value) = typed_array_get_own_property_value(ta, key) {
            result = crate::array::js_array_push_f64(result, value);
        }
    }
    result
}

pub(crate) unsafe fn typed_array_own_enumerable_entries(
    ta: *const TypedArrayHeader,
) -> *mut ArrayHeader {
    if ta.is_null() {
        return crate::array::js_array_alloc(0);
    }
    let owner = ta as usize;
    let len = typed_array_owner_length(owner);
    let names = typed_array_non_index_keys(owner, true);
    let mut result = crate::array::js_array_alloc(len.saturating_add(names.len() as u32));
    for i in 0..len {
        let pair = crate::array::js_array_alloc(2);
        let name = i.to_string();
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let pair = crate::array::js_array_push(pair, crate::JSValue::string_ptr(key));
        let pair = crate::array::js_array_push(
            pair,
            crate::JSValue::number(typed_array_owner_get(owner, i)),
        );
        result = crate::array::js_array_push(result, crate::JSValue::array_ptr(pair));
    }
    for name in names {
        let pair = crate::array::js_array_alloc(2);
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let value = typed_array_get_own_property_value(ta, key)
            .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
        let pair = crate::array::js_array_push(pair, crate::JSValue::string_ptr(key));
        let pair = crate::array::js_array_push(pair, crate::JSValue::from_bits(value.to_bits()));
        result = crate::array::js_array_push(result, crate::JSValue::array_ptr(pair));
    }
    result
}

pub(crate) unsafe fn typed_array_get_own_property_descriptor(
    ta: *const TypedArrayHeader,
    key: *const crate::string::StringHeader,
) -> f64 {
    if ta.is_null() || key.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let Some(name) = string_header_str(key) else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    let owner = ta as usize;
    match typed_array_string_key_kind(name, typed_array_owner_length(owner)) {
        TypedArrayStringKeyKind::InBoundsIndex(index) => crate::object::build_data_descriptor(
            typed_array_owner_get(owner, index),
            true,
            true,
            true,
        ),
        TypedArrayStringKeyKind::IntegerIndex => f64::from_bits(crate::value::TAG_UNDEFINED),
        TypedArrayStringKeyKind::Ordinary => {
            let Some(prop) = typed_array_own_prop_snapshot(owner, name) else {
                return f64::from_bits(crate::value::TAG_UNDEFINED);
            };
            let attrs = crate::object::get_property_attrs(owner, name)
                .unwrap_or(crate::object::PropertyAttrs::new(prop.is_data, true, true));
            if !prop.is_data {
                if let Some(acc) = crate::object::get_accessor_descriptor(owner, name) {
                    let get = if acc.get == 0 {
                        f64::from_bits(crate::value::TAG_UNDEFINED)
                    } else {
                        f64::from_bits(acc.get)
                    };
                    let set = if acc.set == 0 {
                        f64::from_bits(crate::value::TAG_UNDEFINED)
                    } else {
                        f64::from_bits(acc.set)
                    };
                    return crate::object::build_accessor_descriptor(
                        get,
                        set,
                        attrs.enumerable(),
                        attrs.configurable(),
                    );
                }
            }
            crate::object::build_data_descriptor(
                prop.value,
                attrs.writable(),
                attrs.enumerable(),
                attrs.configurable(),
            )
        }
    }
}

pub(crate) unsafe fn typed_array_delete_own_property(
    ta: *mut TypedArrayHeader,
    key: *const crate::string::StringHeader,
) -> i32 {
    if ta.is_null() || key.is_null() {
        return 1;
    }
    let Some(name) = string_header_str(key) else {
        return 1;
    };
    let owner = ta as usize;
    match typed_array_string_key_kind(name, typed_array_owner_length(owner)) {
        TypedArrayStringKeyKind::InBoundsIndex(_) => 0,
        TypedArrayStringKeyKind::IntegerIndex => 1,
        TypedArrayStringKeyKind::Ordinary => {
            if !typed_array_has_ordinary_own_prop(owner, name) {
                return 1;
            }
            if let Some(attrs) = crate::object::get_property_attrs(owner, name) {
                if !attrs.configurable() {
                    return 0;
                }
            }
            remove_typed_array_own_prop(owner, name);
            crate::object::clear_accessor_descriptor(owner, name);
            crate::object::clear_property_attrs(owner, name);
            1
        }
    }
}

pub(crate) fn scan_typed_array_own_props_roots_mut(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
) {
    TYPED_ARRAY_OWN_PROPS.with(|m| {
        for props in m.borrow_mut().values_mut() {
            for prop in props.iter_mut().filter(|prop| prop.is_data) {
                visitor.visit_nanbox_f64_slot(&mut prop.value);
            }
        }
    });
}
