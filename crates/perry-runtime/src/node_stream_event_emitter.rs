use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_set_capture_f64, ClosureHeader,
};
use crate::value::JSValue;

#[path = "node_stream_event_emitter_shape.rs"]
mod shape;
pub(super) use shape::shape_method;
use shape::*;

pub(super) extern "C" fn ns_set_max_listeners(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    value: f64,
) -> f64 {
    set_stream_max_listeners(super::this_value(closure, this), value)
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_set_max_listeners(stream_handle: i64, value: f64) -> f64 {
    set_stream_max_listeners(super::stream_value_from_handle(stream_handle), value)
}

fn set_stream_max_listeners(stream: f64, value: f64) -> f64 {
    let value = validate_max_listeners(value);
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    state_set(
        stream.get_nanbox_f64(),
        &MAX_LISTENERS_SLOT,
        MAX_LISTENERS_KEY,
        value,
    );
    stream.get_nanbox_f64()
}

fn format_max_listeners_received(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n.is_infinite() {
        return if n.is_sign_negative() {
            "-Infinity"
        } else {
            "Infinity"
        }
        .to_string();
    }
    if n.fract() == 0.0 && n.abs() < 1e21 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

fn throw_max_listeners_invalid_type(value: f64) -> ! {
    let message = format!(
        "The \"setMaxListeners\" argument must be of type number. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn throw_max_listeners_out_of_range(n: f64) -> ! {
    let message = format!(
        "The value of \"setMaxListeners\" is out of range. It must be >= 0. Received {}",
        format_max_listeners_received(n)
    );
    crate::fs::validate::throw_range_error_with_code(&message)
}

pub(crate) fn validate_max_listeners(value: f64) -> f64 {
    let js_value = JSValue::from_bits(value.to_bits());
    if !crate::fs::validate::is_numeric(js_value) {
        throw_max_listeners_invalid_type(value);
    }
    let n = if js_value.is_int32() {
        js_value.as_int32() as f64
    } else {
        js_value.as_number()
    };
    if n.is_nan() || n < 0.0 {
        throw_max_listeners_out_of_range(n);
    }
    n
}

pub(super) extern "C" fn ns_get_max_listeners(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    stream_max_listeners(super::this_value(closure, this))
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_get_max_listeners(stream_handle: i64) -> f64 {
    stream_max_listeners(super::stream_value_from_handle(stream_handle))
}

fn stream_max_listeners(stream: f64) -> f64 {
    // node: `_maxListeners === undefined ? defaultMaxListeners : _maxListeners`.
    let max = state_get(stream, &MAX_LISTENERS_SLOT, MAX_LISTENERS_KEY);
    if is_undefined(max) {
        default_max_listeners()
    } else {
        max
    }
}

/// `EventEmitter.defaultMaxListeners`, which a program may reassign. Until
/// the `EventEmitter` export exists nothing can have reassigned it, so a
/// stream-only program reads the default without minting it.
fn default_max_listeners() -> f64 {
    let Some(ctor) =
        crate::object::native_module::minted_native_callable_export("events\0EventEmitter")
    else {
        return DEFAULT_MAX_LISTENERS;
    };
    let jsval = JSValue::from_bits(ctor.to_bits());
    if jsval.is_pointer() {
        let value = crate::closure::closure_get_dynamic_prop(
            jsval.as_pointer::<u8>() as usize,
            "defaultMaxListeners",
        );
        let number = number_of(value);
        if !number.is_nan() {
            return number;
        }
    }
    DEFAULT_MAX_LISTENERS
}

pub(super) extern "C" fn ns_on2(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    event: f64,
    cb: f64,
) -> f64 {
    let stream = super::this_value(closure, this);
    add_stream_listener_for_event(stream, event, cb);
    stream
}

pub(super) extern "C" fn ns_once2(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    event: f64,
    cb: f64,
) -> f64 {
    let stream = super::this_value(closure, this);
    add_stream_listener_for_event_with_options(stream, event, cb, true, false);
    stream
}

pub(super) extern "C" fn ns_prepend_listener2(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    event: f64,
    cb: f64,
) -> f64 {
    let stream = super::this_value(closure, this);
    add_stream_listener_for_event_with_options(stream, event, cb, false, true);
    stream
}

pub(super) extern "C" fn ns_prepend_once_listener2(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    event: f64,
    cb: f64,
) -> f64 {
    let stream = super::this_value(closure, this);
    add_stream_listener_for_event_with_options(stream, event, cb, true, true);
    stream
}

pub(super) extern "C" fn ns_remove_listener2(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    event: f64,
    cb: f64,
) -> f64 {
    let stream = super::this_value(closure, this);
    remove_stream_listener_for_event(stream, event, cb);
    stream
}

pub(super) extern "C" fn ns_off2(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    event: f64,
    cb: f64,
) -> f64 {
    ns_remove_listener2(closure, this, event, cb)
}

pub(super) extern "C" fn ns_remove_all_listeners1(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    event: f64,
) -> f64 {
    let stream = super::this_value(closure, this);
    remove_all_stream_listeners_for_event(stream, event);
    stream
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_on(stream_handle: i64, event: f64, cb: f64) -> f64 {
    let stream = super::stream_value_from_handle(stream_handle);
    if let Some(result) = crate::fs::utf8_stream_on_value(stream, event, cb, false) {
        return result;
    }
    add_stream_listener_for_event(stream, event, cb);
    stream
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_once(stream_handle: i64, event: f64, cb: f64) -> f64 {
    let stream = super::stream_value_from_handle(stream_handle);
    if let Some(result) = crate::fs::utf8_stream_on_value(stream, event, cb, true) {
        return result;
    }
    add_stream_listener_for_event_with_options(stream, event, cb, true, false);
    stream
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_prepend_listener(
    stream_handle: i64,
    event: f64,
    cb: f64,
) -> f64 {
    let stream = super::stream_value_from_handle(stream_handle);
    add_stream_listener_for_event_with_options(stream, event, cb, false, true);
    stream
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_prepend_once_listener(
    stream_handle: i64,
    event: f64,
    cb: f64,
) -> f64 {
    let stream = super::stream_value_from_handle(stream_handle);
    add_stream_listener_for_event_with_options(stream, event, cb, true, true);
    stream
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_remove_listener(
    stream_handle: i64,
    event: f64,
    cb: f64,
) -> f64 {
    let stream = super::stream_value_from_handle(stream_handle);
    if let Some(result) = crate::fs::utf8_stream_off_value(stream, event, cb) {
        return result;
    }
    remove_stream_listener_for_event(stream, event, cb);
    stream
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_off(stream_handle: i64, event: f64, cb: f64) -> f64 {
    js_node_stream_method_remove_listener(stream_handle, event, cb)
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_remove_all_listeners(
    stream_handle: i64,
    event: f64,
) -> f64 {
    let stream = super::stream_value_from_handle(stream_handle);
    if let Some(result) = crate::fs::utf8_stream_remove_all_value(stream, event) {
        return result;
    }
    remove_all_stream_listeners_for_event(stream, event);
    stream
}

/// node's `listenerCount(type, listener)`: with a listener, only the stored
/// entries that are it (or a once wrapper of it) count.
pub(super) extern "C" fn ns_listener_count(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    event: f64,
    listener: f64,
) -> f64 {
    let stream = super::this_value(closure, this);
    let listener_value = JSValue::from_bits(listener.to_bits());
    if listener_value.is_undefined() || listener_value.is_null() {
        return stream_listener_count_for_event(stream, event) as f64;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let stored = stored_listeners(stream, event);
    let stored = scope.root_nanbox_f64_slice(&stored);
    let wanted = listener.to_bits();
    stored
        .iter()
        .filter(|handle| {
            let entry = handle.get_nanbox_f64();
            entry.to_bits() == wanted || unwrap_listener(entry).to_bits() == wanted
        })
        .count() as f64
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_listener_count(stream_handle: i64, event: f64) -> f64 {
    let stream = super::stream_value_from_handle(stream_handle);
    if let Some(result) = crate::fs::utf8_stream_listener_count_value(stream, event) {
        return result;
    }
    stream_listener_count_for_event(stream, event) as f64
}

pub(super) extern "C" fn ns_event_names(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    let stream = super::this_value(closure, this);
    f64::from_bits(JSValue::pointer(stream_event_names_array(stream) as *const u8).bits())
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_event_names(stream_handle: i64) -> i64 {
    stream_event_names_array(super::stream_value_from_handle(stream_handle)) as i64
}

pub(super) extern "C" fn ns_listeners(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    event: f64,
) -> f64 {
    let stream = super::this_value(closure, this);
    f64::from_bits(
        JSValue::pointer(stream_listeners_array_for_event(stream, event, false) as *const u8)
            .bits(),
    )
}

pub(super) extern "C" fn ns_raw_listeners(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    event: f64,
) -> f64 {
    let stream = super::this_value(closure, this);
    f64::from_bits(
        JSValue::pointer(stream_listeners_array_for_event(stream, event, true) as *const u8).bits(),
    )
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_listeners(stream_handle: i64, event: f64) -> i64 {
    stream_listeners_array_for_event(super::stream_value_from_handle(stream_handle), event, false)
        as i64
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_raw_listeners(stream_handle: i64, event: f64) -> i64 {
    stream_listeners_array_for_event(super::stream_value_from_handle(stream_handle), event, true)
        as i64
}

pub(super) fn is_callable_value(value: f64) -> bool {
    let raw = super::raw_ptr_from_value(value);
    raw >= 0x10000 && !crate::closure::get_valid_func_ptr(raw as *const ClosureHeader).is_null()
}

pub(super) fn add_stream_listener_for_event(stream: f64, event: f64, cb: f64) {
    add_stream_listener_for_event_with_options(stream, event, cb, false, false);
}

// ─────────────────────────────────────────────────────────────────
// Listener state is node's own (`lib/events.js`): `this._events` is a
// null-prototype object mapping each event key to ONE listener function or an
// array of them, `this._eventsCount` counts its keys, and a `once` listener is
// stored as a wrapper function whose `.listener` is the original. Every read
// and write goes through ordinary property access on the receiver, so code
// that inspects or edits `_events` directly (readable-stream's
// `prependListener`, ee-first, user code) sees and changes the same state the
// methods use, and the methods themselves live on one shared prototype
// rather than on every instance.
// ─────────────────────────────────────────────────────────────────

const EVENTS_KEY: &[u8] = b"_events";
const EVENTS_COUNT_KEY: &[u8] = b"_eventsCount";
const MAX_LISTENERS_KEY: &[u8] = b"_maxListeners";
const DEFAULT_MAX_LISTENERS: f64 = 10.0;

fn undefined_value() -> f64 {
    f64::from_bits(super::TAG_UNDEFINED)
}

fn is_undefined(value: f64) -> bool {
    value.to_bits() == super::TAG_UNDEFINED
}

/// The bytes of a string value (heap or inline), handed to `f`; `None` for
/// any other value (a symbol, a number to be coerced).
fn with_string_bytes<R>(key: f64, f: impl FnOnce(&[u8]) -> R) -> Option<R> {
    let value = JSValue::from_bits(key.to_bits());
    if value.is_string() {
        let ptr = value.as_string_ptr();
        if ptr.is_null() {
            return None;
        }
        // SAFETY: a string value's pointer names a live string header whose
        // payload follows it.
        let bytes = unsafe {
            std::slice::from_raw_parts(crate::string::string_data(ptr), (*ptr).byte_len as usize)
        };
        return Some(f(bytes));
    }
    if value.is_short_string() {
        let mut buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let n = value.short_string_to_buf(&mut buf);
        return Some(f(&buf[..n]));
    }
    None
}

/// `target[name]` answered by the shapes: an ordinary data property on the
/// chain, or a definite miss at a null `[[Prototype]]`. `None` when the
/// shapes cannot answer (an accessor, an exotic receiver, a native
/// prototype), and the caller takes the full `[[Get]]`. Reads no meta and
/// allocates nothing.
fn shape_get(target: f64, name: &[u8]) -> Option<f64> {
    let value = JSValue::from_bits(target.to_bits());
    if !value.is_pointer() {
        return None;
    }
    // SAFETY: the lookup validates the address (arena membership, header)
    // before it reads anything.
    unsafe { crate::object::native_get::try_data_lookup_bytes(value, name) }
        .map(|found| found.map_or_else(undefined_value, |v| f64::from_bits(v.bits())))
}

/// `target[key] = value` for a key `target` already owns as plain writable
/// data: one key-list probe and a barriered slot store. `false` sends the
/// caller to the full `[[Set]]`.
fn shape_set_existing(target: f64, name: &[u8], value: f64) -> bool {
    let jsval = JSValue::from_bits(target.to_bits());
    if !jsval.is_pointer() {
        return false;
    }
    let addr = jsval.as_pointer::<u8>() as usize;
    if !crate::value::addr_class::is_above_handle_band(addr) {
        return false;
    }
    // SAFETY: an address above the handle band; the reader validates it.
    let Some(header) = (unsafe { crate::value::addr_class::try_read_gc_header(addr) }) else {
        return false;
    };
    // The overwrite consults the key's VALUE only to vet an own descriptor;
    // an object with none never reaches that check.
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header._reserved & crate::gc::OBJ_FLAG_HAS_DESCRIPTORS != 0
    {
        return false;
    }
    // SAFETY: a live ordinary object (checked above); the overwrite does not
    // allocate in the GC heap.
    unsafe {
        crate::object::try_existing_own_data_overwrite_by_content(
            addr as *mut crate::object::ObjectHeader,
            undefined_value(),
            name,
            value,
        )
    }
}

/// `target[key]`: the full [[Get]] (prototype chain, accessors, proxies),
/// after the shapes had their say.
fn get_key(target: f64, key: f64) -> f64 {
    if let Some(Some(found)) = with_string_bytes(key, |bytes| own_get(target, key.to_bits(), bytes))
    {
        return found;
    }
    if let Some(Some(found)) = with_string_bytes(key, |bytes| shape_get(target, bytes)) {
        return found;
    }
    unsafe { crate::object::js_object_get_property_key(target, key) }
}

/// `events[name]` for one of node's meta-event names (`newListener`,
/// `removeListener`), answered by the shape when it can.
fn get_event_named(events: f64, name: &[u8]) -> f64 {
    own_get(events, 0, name).unwrap_or_else(|| get_named(events, name))
}

/// `target[key] = value`: the full [[Set]], after the shapes had their say.
fn set_key(target: f64, key: f64, value: f64) {
    if with_string_bytes(key, |bytes| shape_set(target, key.to_bits(), bytes, value)) == Some(true)
    {
        return;
    }
    if with_string_bytes(key, |bytes| shape_set_existing(target, bytes, value)) == Some(true) {
        return;
    }
    // A key deleted by `off` and re-added by the next `on` refills its
    // tombstoned slot on the same shape (the lane a computed `[[Set]]` miss
    // takes first); anything else is the ordinary `[[Set]]`.
    let target_value = JSValue::from_bits(target.to_bits());
    if target_value.is_pointer() {
        let obj = target_value.as_pointer::<crate::object::ObjectHeader>() as *mut _;
        if crate::object::try_readd_stable_tombstone(obj, key, value).is_some() {
            return;
        }
    }
    unsafe {
        crate::object::js_object_set_property_key(target, key, value);
    }
}

/// A runtime-owned property name, interned: allocates only on the first use
/// per thread, then a hash probe returning the canonical (rooted) string.
fn name_key(name: &[u8]) -> f64 {
    f64::from_bits(JSValue::string_ptr(crate::string::intern_ascii_literal(name) as *mut _).bits())
}

fn get_named(target: f64, name: &[u8]) -> f64 {
    // An emitter's state keys are its own: the receiver's own key list
    // answers them for a subclass instance too, which the chain lookup
    // below refuses (so it is not tried for one).
    match own_data_lookup(target, name) {
        Some(Some(own)) => return own,
        Some(None) if declared_class_instance(target) => {
            if let Some(found) = chain_own_lookup(target, name) {
                return found;
            }
        }
        _ => {
            if let Some(found) = shape_get(target, name) {
                return found;
            }
        }
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let key = name_key(name);
    unsafe { crate::object::js_object_get_property_key(target.get_nanbox_f64(), key) }
}

fn set_named(target: f64, name: &[u8], value: f64) {
    // An own data key is overwritten in place; an absent one is an add,
    // which only the full `[[Set]]` may make (the chain may intercept it).
    if shape_set_existing(target, name, value) {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let value = scope.root_nanbox_f64(value);
    let key = name_key(name);
    unsafe {
        crate::object::js_object_set_property_key(
            target.get_nanbox_f64(),
            key,
            value.get_nanbox_f64(),
        );
    }
}

/// `target[name]` for a key `target` lacks, walking its `[[Prototype]]`s
/// and asking each one's own key list (`a class X extends EventEmitter`
/// instance's `_maxListeners` lives on `EventEmitter.prototype`). `None`
/// when some hop cannot answer by its shape (the full `[[Get]]` then runs).
fn chain_own_lookup(target: f64, name: &[u8]) -> Option<f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let mut hop = scope.root_nanbox_f64(target);
    for _ in 0..16 {
        let proto = crate::object::js_object_get_prototype_of(hop.get_nanbox_f64());
        let proto_value = JSValue::from_bits(proto.to_bits());
        if proto_value.is_null() {
            return Some(undefined_value());
        }
        if !proto_value.is_pointer() {
            return None;
        }
        match own_data_lookup(proto, name)? {
            Some(found) => return Some(found),
            None => hop = scope.root_nanbox_f64(proto),
        }
    }
    None
}

/// An instance of a declared (compiled) class: its chain is the class
/// registry's, which the shape-chain lookup does not walk.
fn declared_class_instance(target: f64) -> bool {
    let jsval = JSValue::from_bits(target.to_bits());
    if !jsval.is_pointer() {
        return false;
    }
    // SAFETY: only called after `own_data_lookup` validated the receiver as
    // a live ordinary object.
    let class_id = unsafe { (*jsval.as_pointer::<crate::object::ObjectHeader>()).class_id };
    class_id != 0
        && class_id < crate::object::SYNTHETIC_CLASS_ID_BASE
        && !crate::object::is_anon_shape_class_id(class_id)
}

/// `target`'s OWN data property `name`, from its shape's key list alone (any
/// class): `Some(Some(v))` present, `Some(None)` absent, `None` when the
/// shape cannot answer (an exotic or dictionary receiver, a possible own
/// accessor for the key).
fn own_data_lookup(target: f64, name: &[u8]) -> Option<Option<f64>> {
    own_data_slot(target, name).map(|own| {
        own.map(|(obj, slot, live)| {
            // SAFETY: `own_data_slot` resolved `slot` from `obj`'s shape.
            let value =
                unsafe { crate::object::field_get_set::object_field_at_with_live(obj, slot, live) };
            f64::from_bits(value.bits())
        })
    })
}

/// [`own_data_lookup`]'s slot: the object, the key's slot and the shape's
/// live inline bound.
fn own_data_slot(
    target: f64,
    name: &[u8],
) -> Option<Option<(*const crate::object::ObjectHeader, u32, u32)>> {
    let jsval = JSValue::from_bits(target.to_bits());
    if !jsval.is_pointer() {
        return None;
    }
    let addr = jsval.as_pointer::<u8>() as usize;
    if !crate::value::addr_class::is_plausible_heap_addr(addr)
        || crate::arena::classify_heap_generation(addr) == crate::arena::HeapGeneration::Unknown
    {
        return None;
    }
    unsafe {
        let header = crate::value::addr_class::try_read_gc_header_known_plausible(addr)?;
        if header.obj_type != crate::gc::GC_TYPE_OBJECT
            || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return None;
        }
        let obj = addr as *const crate::object::ObjectHeader;
        // A descriptor for THIS key (an attribute or an accessor) is the
        // descriptor tables' to answer; other keys' descriptors do not matter.
        if header._reserved & crate::gc::OBJ_FLAG_HAS_DESCRIPTORS != 0
            && !(crate::object::key_attrs::attrs_live_in_keys(addr)
                && crate::object::key_attrs::object_key_entry(obj, name) == 0)
        {
            return None;
        }
        let meta = (*obj).meta;
        if (!meta.is_null() && (*meta).elements != 0)
            || crate::object::key_attrs::object_key_is_accessor(obj, name)
        {
            return None;
        }
        if crate::object::dictionary::is_dictionary(obj) {
            return None;
        }
        let shape = crate::object::shapes::object_shape_descriptor(obj)?;
        if !shape.object_kind.is_ordinary_layout() {
            return None;
        }
        let keys = shape.keys as usize as *const crate::array::ArrayHeader;
        if keys.is_null() {
            return (shape.logical_key_count == 0).then_some(None);
        }
        let Some(slot) =
            crate::object::keys_find_slot_by_bytes_resolved(keys, shape.logical_key_count, name)
        else {
            return Some(None);
        };
        Some(Some((obj, slot, shape.live_inline_slot_count)))
    }
}

fn is_object_value(value: f64) -> bool {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_pointer() {
        return false;
    }
    let addr = jsval.as_pointer::<u8>() as usize;
    // An arena cell carries its GC header; anything else (a headerless
    // buffer, a handle) takes the classifying probe.
    if crate::value::addr_class::is_plausible_heap_addr(addr)
        && crate::arena::classify_heap_generation(addr) != crate::arena::HeapGeneration::Unknown
    {
        // SAFETY: plausible and arena-owned, so the header is readable.
        return unsafe { crate::value::addr_class::try_read_gc_header_known_plausible(addr) }
            .is_some_and(|header| header.obj_type == crate::gc::GC_TYPE_OBJECT);
    }
    super::object_ptr_from_value(value).is_some()
}

fn is_array_value(value: f64) -> bool {
    crate::array::js_array_is_array(value).to_bits() == super::TAG_TRUE
}

fn number_of(value: f64) -> f64 {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_int32() {
        js.as_int32() as f64
    } else if js.is_number() {
        value
    } else {
        f64::NAN
    }
}

/// `{ __proto__: null }`, node's empty `_events`, born with its first
/// two listener slots already live (the physical allocation floor), so a
/// rollback reinstalls the exact parent without widening on the next add.
pub(crate) fn new_events_object() -> f64 {
    crate::value::js_nanbox_pointer(crate::object::js_object_alloc_null_proto(0, 2) as i64)
}

fn reset_events(target: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let events = new_events_object();
    if reset_events_fast(target.get_nanbox_f64(), events) {
        return;
    }
    state_set(target.get_nanbox_f64(), &EVENTS_SLOT, EVENTS_KEY, events);
    state_set(
        target.get_nanbox_f64(),
        &EVENTS_COUNT_SLOT,
        EVENTS_COUNT_KEY,
        0.0,
    );
}

fn adjust_events_count(target: f64, delta: f64) -> f64 {
    // An own plain `_eventsCount` (every emitter's): read and write its slot.
    if let Some((obj, slot, flags)) = state_slot(target, &EVENTS_COUNT_SLOT, EVENTS_COUNT_KEY) {
        if flags & crate::gc::OBJ_FLAG_FROZEN == 0 {
            // SAFETY: `state_slot` resolved the key's own inline data slot on
            // the live object; nothing below allocates.
            let count = number_of(f64::from_bits(unsafe { slot_bits(obj, slot) })) + delta;
            unsafe { crate::object::store_object_field_slot(obj, slot as usize, count.to_bits()) };
            return count;
        }
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let count = number_of(get_named(target.get_nanbox_f64(), EVENTS_COUNT_KEY)) + delta;
    set_named(target.get_nanbox_f64(), EVENTS_COUNT_KEY, count);
    count
}

/// The receiver's `_events` when it is an object.
fn events_of(target: f64) -> Option<f64> {
    let events = state_get(target, &EVENTS_SLOT, EVENTS_KEY);
    is_object_value(events).then_some(events)
}

/// node's `EventEmitter.init`: give the receiver its own `_events` (unless it
/// already has one that is not merely inherited), `_eventsCount` and
/// `_maxListeners`. Run by `super()` of a class extending EventEmitter and by
/// `EventEmitter.call(this)`.
pub(crate) fn init_event_emitter_state(target: f64) {
    if !is_object_value(target) {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    // node: reset when `this._events === undefined || this._events ===
    // ObjectGetPrototypeOf(this)._events`. Without an own `_events` the read
    // IS the prototype's, so both arms hold and the (slow, inherited) reads
    // are skipped; only an own `_events` needs comparing.
    let has_own = match own_data_lookup(target.get_nanbox_f64(), EVENTS_KEY) {
        Some(own) => own.is_some(),
        None => {
            crate::object::js_object_has_own(target.get_nanbox_f64(), name_key(EVENTS_KEY))
                .to_bits()
                == super::TAG_TRUE
        }
    };
    let reset = !has_own || {
        let events = scope.root_nanbox_f64(get_named(target.get_nanbox_f64(), EVENTS_KEY));
        is_undefined(events.get_nanbox_f64()) || {
            let proto = crate::object::js_object_get_prototype_of(target.get_nanbox_f64());
            is_object_value(proto)
                && get_named(proto, EVENTS_KEY).to_bits() == events.get_nanbox_f64().to_bits()
        }
    };
    if reset {
        reset_events(target.get_nanbox_f64());
    }
    let max = state_get(
        target.get_nanbox_f64(),
        &MAX_LISTENERS_SLOT,
        MAX_LISTENERS_KEY,
    );
    let max = if crate::value::js_is_truthy(max) != 0 {
        max
    } else {
        undefined_value()
    };
    state_set(
        target.get_nanbox_f64(),
        &MAX_LISTENERS_SLOT,
        MAX_LISTENERS_KEY,
        max,
    );
}

/// The data properties node's `EventEmitter.prototype` carries ahead of its
/// methods: `_events: undefined`, `_eventsCount: 0`, `_maxListeners: undefined`.
pub(crate) fn install_event_emitter_prototype_state(proto: f64) {
    set_named(proto, EVENTS_KEY, undefined_value());
    set_named(proto, EVENTS_COUNT_KEY, 0.0);
    set_named(proto, MAX_LISTENERS_KEY, undefined_value());
}

/// Call `target[name](...args)` with `this = target`, as node's emitter does
/// for `this.emit('newListener', …)`, `this.removeListener(…)` and
/// `this.removeAllListeners(…)`, so a subclass override sees those calls.
/// `None` when the receiver has no callable `name`.
fn call_method(target: f64, name: &[u8], args: &[f64]) -> Option<f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let arg_handles = scope.root_nanbox_f64_slice(args);
    let method = get_named(target.get_nanbox_f64(), name);
    if !is_callable_value(method) {
        return None;
    }
    let live_args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
    Some(unsafe {
        crate::closure::native_call_value_this(
            method,
            crate::closure::JsThis::from_f64(target.get_nanbox_f64()),
            live_args.as_ptr(),
            live_args.len(),
        )
    })
}

fn emit_via_method(target: f64, args: &[f64]) {
    if call_method(target, b"emit", args).is_none() {
        if let Some((event, rest)) = args.split_first() {
            let _ = emit_stream_event(target, *event, rest);
        }
    }
}

/// `listener.listener ?? listener` — a once wrapper's original.
fn unwrap_listener(listener: f64) -> f64 {
    if !is_callable_value(listener) {
        return listener;
    }
    // A function's own `listener` (a once wrapper's original); reading it
    // allocates nothing.
    let inner =
        crate::closure::closure_get_dynamic_prop(super::raw_ptr_from_value(listener), "listener");
    if is_undefined(inner) || inner.to_bits() == crate::value::TAG_NULL {
        listener
    } else {
        inner
    }
}

/// The body of node's `onceWrapper`: captures `[target, type, listener,
/// fired]`; the first call removes the wrapper and forwards to the listener.
extern "C" fn ns_once_wrapper(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    rest: f64,
) -> f64 {
    if closure.is_null() {
        return undefined_value();
    }
    if crate::value::js_is_truthy(js_closure_get_capture_f64(closure, 3)) != 0 {
        return undefined_value();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let wrapper = scope.root_nanbox_f64(super::box_pointer(closure as *const u8));
    let target = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 0));
    let event = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 1));
    let listener = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 2));
    let args = {
        let arr = super::raw_ptr_from_value(rest) as *const crate::array::ArrayHeader;
        let len = if arr.is_null() || !is_array_value(rest) {
            0
        } else {
            crate::array::js_array_length(arr)
        };
        (0..len)
            .map(|i| crate::array::js_array_get_f64(arr, i))
            .collect::<Vec<f64>>()
    };
    let arg_handles = scope.root_nanbox_f64_slice(&args);
    let removal = [event.get_nanbox_f64(), wrapper.get_nanbox_f64()];
    if call_method(target.get_nanbox_f64(), b"removeListener", &removal).is_none() {
        remove_stream_listener_for_event(
            target.get_nanbox_f64(),
            event.get_nanbox_f64(),
            wrapper.get_nanbox_f64(),
        );
    }
    let wrapper_ptr = super::raw_ptr_from_value(wrapper.get_nanbox_f64()) as *mut ClosureHeader;
    js_closure_set_capture_f64(wrapper_ptr, 3, f64::from_bits(super::TAG_TRUE));
    let live_args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
    if !is_callable_value(listener.get_nanbox_f64()) {
        return undefined_value();
    }
    unsafe {
        crate::closure::native_call_value_this(
            listener.get_nanbox_f64(),
            crate::closure::JsThis::from_f64(target.get_nanbox_f64()),
            live_args.as_ptr(),
            live_args.len(),
        )
    }
}

/// node's `_onceWrap(target, type, listener)`.
fn once_wrap(target: f64, event: f64, listener: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let event = scope.root_nanbox_f64(event);
    let listener = scope.root_nanbox_f64(listener);
    let wrapper = js_closure_alloc(crate::fn_info!(ns_once_wrapper, 1; with_rest(0)), 4);
    js_closure_set_capture_f64(wrapper, 0, target.get_nanbox_f64());
    js_closure_set_capture_f64(wrapper, 1, event.get_nanbox_f64());
    js_closure_set_capture_f64(wrapper, 2, listener.get_nanbox_f64());
    js_closure_set_capture_f64(wrapper, 3, f64::from_bits(super::TAG_FALSE));
    let wrapper = scope.root_nanbox_f64(super::box_pointer(wrapper as *const u8));
    set_named(
        wrapper.get_nanbox_f64(),
        b"listener",
        listener.get_nanbox_f64(),
    );
    wrapper.get_nanbox_f64()
}

fn add_stream_listener_for_event_with_options(
    stream: f64,
    event: f64,
    cb: f64,
    once: bool,
    prepend: bool,
) {
    if !is_callable_value(cb) {
        throw_invalid_listener_type(cb);
    }
    if !once && add_first_listener_fast(stream, event, cb) {
        listener_added(stream, event);
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(stream);
    let event = scope.root_nanbox_f64(event);
    let cb = scope.root_nanbox_f64(cb);
    let stored = if once {
        once_wrap(
            target.get_nanbox_f64(),
            event.get_nanbox_f64(),
            cb.get_nanbox_f64(),
        )
    } else {
        cb.get_nanbox_f64()
    };
    let stored = scope.root_nanbox_f64(stored);

    let events = match events_of(target.get_nanbox_f64()) {
        None => {
            reset_events(target.get_nanbox_f64());
            events_of(target.get_nanbox_f64())
        }
        Some(events) => {
            if !is_undefined(get_event_named(events, b"newListener")) {
                let announced = if once {
                    cb.get_nanbox_f64()
                } else {
                    unwrap_listener(cb.get_nanbox_f64())
                };
                let meta = name_key(b"newListener");
                emit_via_method(
                    target.get_nanbox_f64(),
                    &[meta, event.get_nanbox_f64(), announced],
                );
                // A `newListener` listener may have replaced `_events`.
                events_of(target.get_nanbox_f64())
            } else {
                Some(events)
            }
        }
    };
    let Some(events) = events else {
        return;
    };
    let events = scope.root_nanbox_f64(events);
    let existing = scope.root_nanbox_f64(get_key(events.get_nanbox_f64(), event.get_nanbox_f64()));
    if is_undefined(existing.get_nanbox_f64()) {
        set_key(
            events.get_nanbox_f64(),
            event.get_nanbox_f64(),
            stored.get_nanbox_f64(),
        );
        adjust_events_count(target.get_nanbox_f64(), 1.0);
    } else if is_array_value(existing.get_nanbox_f64()) {
        let arr =
            super::raw_ptr_from_value(existing.get_nanbox_f64()) as *mut crate::array::ArrayHeader;
        let grown = if prepend {
            crate::array::js_array_unshift_f64(arr, stored.get_nanbox_f64())
        } else {
            crate::array::js_array_push_f64(arr, stored.get_nanbox_f64())
        };
        let grown = scope.root_nanbox_f64(super::box_pointer(grown as *const u8));
        if grown.get_nanbox_u64() != existing.get_nanbox_u64() {
            set_key(
                events.get_nanbox_f64(),
                event.get_nanbox_f64(),
                grown.get_nanbox_f64(),
            );
        }
        check_listener_limit(
            target.get_nanbox_f64(),
            event.get_nanbox_f64(),
            grown.get_nanbox_f64(),
        );
    } else {
        let mut pair = crate::array::js_array_alloc(2);
        let (first, second) = if prepend {
            (stored.get_nanbox_f64(), existing.get_nanbox_f64())
        } else {
            (existing.get_nanbox_f64(), stored.get_nanbox_f64())
        };
        pair = crate::array::js_array_push_f64(pair, first);
        pair = crate::array::js_array_push_f64(pair, second);
        let pair = scope.root_nanbox_f64(super::box_pointer(pair as *const u8));
        set_key(
            events.get_nanbox_f64(),
            event.get_nanbox_f64(),
            pair.get_nanbox_f64(),
        );
        check_listener_limit(
            target.get_nanbox_f64(),
            event.get_nanbox_f64(),
            pair.get_nanbox_f64(),
        );
    }

    listener_added(target.get_nanbox_f64(), event.get_nanbox_f64());
}

/// A stream starts flowing (`data`) or reading (`readable`) once it has a
/// listener for it.
fn listener_added(target: f64, event: f64) {
    if super::string_value_eq(event, b"data") {
        super::readable_data_listener_added(target);
    } else if super::string_value_eq(event, b"readable") {
        super::readable_listener_added(target);
    }
}

/// node's `_addListener` leak check: a listener list that outgrows the
/// emitter's maximum (`getMaxListeners()`, 0 = unlimited) warns once, through
/// `process.emitWarning`, with a `MaxListenersExceededWarning` naming the
/// emitter, the event and the count. The list's `warned` flag is node's.
fn check_listener_limit(target: f64, event: f64, list: f64) {
    let len = if is_array_value(list) {
        crate::array::js_array_length(super::raw_ptr_from_value(list) as *const _) as f64
    } else {
        return;
    };
    let max = stream_max_listeners(target);
    if !(max > 0.0) || len <= max {
        return;
    }
    if crate::value::js_is_truthy(get_named(list, b"warned")) != 0 {
        return;
    }
    warn_listener_limit(target, event, list, len, max);
}

#[cold]
#[inline(never)]
fn warn_listener_limit(target: f64, event: f64, list: f64, len: f64, max: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let event = scope.root_nanbox_f64(event);
    let list = scope.root_nanbox_f64(list);
    set_named(list.get_nanbox_f64(), b"warned", bool_value(true));
    let type_name = value_text(event.get_nanbox_f64());
    // node: `inspect(target, { depth: -1 })`, e.g. `[EventEmitter]`.
    let options = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 1) as i64,
    ));
    set_named(options.get_nanbox_f64(), b"depth", -1.0);
    let shown = value_text(crate::builtins::js_util_inspect(
        target.get_nanbox_f64(),
        options.get_nanbox_f64(),
    ));
    let text = format!(
        "Possible EventEmitter memory leak detected. {} {type_name} listeners added to {shown}. MaxListeners is {}. Use emitter.setMaxListeners() to increase limit",
        len as u64,
        format_max_listeners_received(max)
    );
    let message = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    let warning = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::error::js_error_new_with_message(message) as i64,
    ));
    let name = b"MaxListenersExceededWarning";
    let name = scope.root_nanbox_f64(f64::from_bits(
        JSValue::string_ptr(crate::string::js_string_from_bytes(
            name.as_ptr(),
            name.len() as u32,
        ))
        .bits(),
    ));
    set_named(warning.get_nanbox_f64(), b"name", name.get_nanbox_f64());
    set_named(
        warning.get_nanbox_f64(),
        b"emitter",
        target.get_nanbox_f64(),
    );
    set_named(warning.get_nanbox_f64(), b"type", event.get_nanbox_f64());
    set_named(warning.get_nanbox_f64(), b"count", len);
    // node: `process.emitWarning(w)` with the Error itself, which the
    // 'warning' listeners receive as is.
    crate::process::schedule_warning(
        warning.get_nanbox_f64(),
        "MaxListenersExceededWarning",
        "",
        &text,
        "",
    );
}

/// `String(value)` as Rust text (a symbol renders as `Symbol(desc)`).
fn value_text(value: f64) -> String {
    let text = if unsafe { crate::symbol::js_is_symbol(value) } != 0 {
        unsafe { crate::symbol::js_symbol_to_string(value) as *const crate::string::StringHeader }
    } else {
        crate::value::js_jsvalue_to_string(value) as *const crate::string::StringHeader
    };
    if text.is_null() {
        return String::new();
    }
    let value = f64::from_bits(JSValue::string_ptr(text as *mut _).bits());
    with_string_bytes(value, |bytes| String::from_utf8_lossy(bytes).into_owned())
        .unwrap_or_default()
}

/// node's `checkListener`: `TypeError [ERR_INVALID_ARG_TYPE]: The "listener"
/// argument must be of type function. Received …`.
#[cold]
fn throw_invalid_listener_type(listener: f64) -> ! {
    const NAME: &[u8] = b"listener";
    // SAFETY: a static ASCII name; the validator throws for a non-function.
    unsafe {
        crate::fs::validate::js_validate_event_listener(
            listener.to_bits() as i64,
            NAME.as_ptr(),
            NAME.len() as u32,
        );
    }
    // The validator accepted a value `is_callable_value` refused: still not
    // a listener this emitter can call.
    let msg = b"The \"listener\" argument must be of type function";
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    crate::node_submodules::register_error_code(s, "ERR_INVALID_ARG_TYPE");
    let err = crate::error::js_typeerror_new(s);
    let bits = JSValue::pointer(err as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(bits))
}

fn emit_remove_listener_if_watched(target: f64, events: f64, event: f64, listener: f64) {
    if is_undefined(get_event_named(events, b"removeListener")) {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let event = scope.root_nanbox_f64(event);
    let listener = scope.root_nanbox_f64(listener);
    let meta = name_key(b"removeListener");
    emit_via_method(
        target.get_nanbox_f64(),
        &[meta, event.get_nanbox_f64(), listener.get_nanbox_f64()],
    );
}

/// node's `removeListener(type, listener)`. True when a listener was removed.
pub(super) fn remove_stream_listener_for_event(stream: f64, event: f64, cb: f64) -> bool {
    if !is_callable_value(cb) {
        throw_invalid_listener_type(cb);
    }
    if remove_only_listener_fast(stream, event, cb) {
        return true;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(stream);
    let event = scope.root_nanbox_f64(event);
    let cb = scope.root_nanbox_f64(cb);
    let Some(events) = events_of(target.get_nanbox_f64()) else {
        return false;
    };
    let events = scope.root_nanbox_f64(events);
    let list = scope.root_nanbox_f64(get_key(events.get_nanbox_f64(), event.get_nanbox_f64()));
    if is_undefined(list.get_nanbox_f64()) {
        return false;
    }
    let cb_bits = cb.get_nanbox_f64().to_bits();
    if is_callable_value(list.get_nanbox_f64()) {
        // node: `list === listener || list.listener === listener`; the
        // wrapper's `.listener` is read only when the first compare fails.
        if list.get_nanbox_f64().to_bits() != cb_bits
            && unwrap_listener(list.get_nanbox_f64()).to_bits() != cb_bits
        {
            return false;
        }
        if adjust_events_count(target.get_nanbox_f64(), -1.0) == 0.0 {
            reset_events(target.get_nanbox_f64());
        } else {
            let _ = crate::object::js_object_delete_dynamic_value(
                events.get_nanbox_f64(),
                event.get_nanbox_f64(),
            );
            // node announces `list.listener || listener`.
            if !is_undefined(get_event_named(events.get_nanbox_f64(), b"removeListener")) {
                let announced = unwrap_listener(list.get_nanbox_f64());
                emit_remove_listener_if_watched(
                    target.get_nanbox_f64(),
                    events.get_nanbox_f64(),
                    event.get_nanbox_f64(),
                    announced,
                );
            }
        }
        return true;
    }
    if !is_array_value(list.get_nanbox_f64()) {
        return false;
    }
    let arr = super::raw_ptr_from_value(list.get_nanbox_f64()) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    let mut position = None;
    for i in (0..len).rev() {
        let arr =
            super::raw_ptr_from_value(list.get_nanbox_f64()) as *const crate::array::ArrayHeader;
        let item = crate::array::js_array_get_f64(arr, i);
        if item.to_bits() == cb_bits || unwrap_listener(item).to_bits() == cb_bits {
            position = Some(i);
            break;
        }
    }
    let Some(position) = position else {
        return false;
    };
    let arr = super::raw_ptr_from_value(list.get_nanbox_f64()) as *mut crate::array::ArrayHeader;
    // `js_array_splice` returns the DELETED elements and reports the edited
    // (possibly reallocated) receiver through its out-parameter.
    let mut kept: *mut crate::array::ArrayHeader = std::ptr::null_mut();
    let _deleted =
        crate::array::js_array_splice(arr, position as i32, 1, std::ptr::null(), 0, &mut kept);
    let kept_value = scope.root_nanbox_f64(super::box_pointer(kept as *const u8));
    let kept =
        super::raw_ptr_from_value(kept_value.get_nanbox_f64()) as *const crate::array::ArrayHeader;
    if crate::array::js_array_length(kept) == 1 {
        let only = crate::array::js_array_get_f64(kept, 0);
        set_key(events.get_nanbox_f64(), event.get_nanbox_f64(), only);
    } else if kept as usize != arr as usize {
        set_key(
            events.get_nanbox_f64(),
            event.get_nanbox_f64(),
            kept_value.get_nanbox_f64(),
        );
    }
    emit_remove_listener_if_watched(
        target.get_nanbox_f64(),
        events.get_nanbox_f64(),
        event.get_nanbox_f64(),
        cb.get_nanbox_f64(),
    );
    true
}

/// node's `removeAllListeners([type])`; `undefined` stands for "no argument".
fn remove_all_stream_listeners_for_event(stream: f64, event: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(stream);
    let event = scope.root_nanbox_f64(event);
    let Some(events) = events_of(target.get_nanbox_f64()) else {
        return;
    };
    let events = scope.root_nanbox_f64(events);
    let all = is_undefined(event.get_nanbox_f64());
    if is_undefined(get_event_named(events.get_nanbox_f64(), b"removeListener")) {
        if all {
            reset_events(target.get_nanbox_f64());
        } else if !is_undefined(get_key(events.get_nanbox_f64(), event.get_nanbox_f64())) {
            if adjust_events_count(target.get_nanbox_f64(), -1.0) == 0.0 {
                reset_events(target.get_nanbox_f64());
            } else {
                let _ = crate::object::js_object_delete_dynamic_value(
                    events.get_nanbox_f64(),
                    event.get_nanbox_f64(),
                );
            }
        }
        return;
    }
    if all {
        let keys = crate::proxy::js_reflect_own_keys(events.get_nanbox_f64());
        let keys = scope.root_nanbox_f64(keys);
        let keys_arr =
            super::raw_ptr_from_value(keys.get_nanbox_f64()) as *const crate::array::ArrayHeader;
        let len = crate::array::js_array_length(keys_arr);
        for i in 0..len {
            let keys_arr = super::raw_ptr_from_value(keys.get_nanbox_f64())
                as *const crate::array::ArrayHeader;
            let key = crate::array::js_array_get_f64(keys_arr, i);
            if super::string_value_eq(key, b"removeListener") {
                continue;
            }
            remove_all_via_method(target.get_nanbox_f64(), key);
        }
        let meta = name_key(b"removeListener");
        remove_all_via_method(target.get_nanbox_f64(), meta);
        reset_events(target.get_nanbox_f64());
        return;
    }
    let listeners = scope.root_nanbox_f64(get_key(events.get_nanbox_f64(), event.get_nanbox_f64()));
    if is_callable_value(listeners.get_nanbox_f64()) {
        remove_via_method(
            target.get_nanbox_f64(),
            event.get_nanbox_f64(),
            listeners.get_nanbox_f64(),
        );
    } else if is_array_value(listeners.get_nanbox_f64()) {
        let arr = super::raw_ptr_from_value(listeners.get_nanbox_f64())
            as *const crate::array::ArrayHeader;
        let len = crate::array::js_array_length(arr);
        for i in (0..len).rev() {
            let arr = super::raw_ptr_from_value(listeners.get_nanbox_f64())
                as *const crate::array::ArrayHeader;
            if i >= crate::array::js_array_length(arr) {
                continue;
            }
            let item = crate::array::js_array_get_f64(arr, i);
            remove_via_method(target.get_nanbox_f64(), event.get_nanbox_f64(), item);
        }
    }
}

fn remove_via_method(target: f64, event: f64, listener: f64) {
    if call_method(target, b"removeListener", &[event, listener]).is_none() {
        remove_stream_listener_for_event(target, event, listener);
    }
}

fn remove_all_via_method(target: f64, event: f64) {
    if call_method(target, b"removeAllListeners", &[event]).is_none() {
        remove_all_stream_listeners_for_event(target, event);
    }
}

/// The listener (functions or once wrappers) stored for `event`, in order.
fn stored_listeners(target: f64, event: f64) -> Vec<f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let event = scope.root_nanbox_f64(event);
    let Some(events) = events_of(target) else {
        return Vec::new();
    };
    let list = get_key(events, event.get_nanbox_f64());
    if is_undefined(list) {
        return Vec::new();
    }
    if is_array_value(list) {
        let arr = super::raw_ptr_from_value(list) as *const crate::array::ArrayHeader;
        let len = crate::array::js_array_length(arr);
        return (0..len)
            .map(|i| crate::array::js_array_get_f64(arr, i))
            .collect();
    }
    if is_callable_value(list) {
        return vec![list];
    }
    Vec::new()
}

pub(super) fn stream_listener_count_for_event(stream: f64, event: f64) -> usize {
    if let Some(list) = listeners_of_fast(stream, event) {
        return listener_list_len(list);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let event = scope.root_nanbox_f64(event);
    let Some(events) = events_of(stream) else {
        return 0;
    };
    listener_list_len(get_key(events, event.get_nanbox_f64()))
}

/// How many listeners one `_events` entry holds.
fn listener_list_len(list: f64) -> usize {
    if is_undefined(list) {
        0
    } else if is_callable_value(list) {
        1
    } else if is_array_value(list) {
        crate::array::js_array_length(super::raw_ptr_from_value(list) as *const _) as usize
    } else {
        0
    }
}

/// node's `eventNames()`: `Reflect.ownKeys(this._events)` while any remain.
fn stream_event_names_array(stream: f64) -> *mut crate::array::ArrayHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(stream);
    let count = number_of(state_get(
        target.get_nanbox_f64(),
        &EVENTS_COUNT_SLOT,
        EVENTS_COUNT_KEY,
    ));
    match events_of(target.get_nanbox_f64()) {
        Some(events) if count > 0.0 => {
            let keys = crate::proxy::js_reflect_own_keys(events);
            super::raw_ptr_from_value(keys) as *mut crate::array::ArrayHeader
        }
        _ => crate::array::js_array_alloc(0),
    }
}

/// `listeners(type)` (unwrapped) or `rawListeners(type)` (once wrappers kept).
fn stream_listeners_array_for_event(
    stream: f64,
    event: f64,
    raw: bool,
) -> *mut crate::array::ArrayHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stored = stored_listeners(stream, event);
    let stored = scope.root_nanbox_f64_slice(&stored);
    let mut values = Vec::with_capacity(stored.len());
    for handle in &stored {
        let listener = handle.get_nanbox_f64();
        values.push(if raw {
            listener
        } else {
            unwrap_listener(listener)
        });
    }
    let values = scope.root_nanbox_f64_slice(&values);
    // `js_array_alloc` reserves the capacity, so no push below allocates and
    // the rooted values stay current across the loop.
    let mut out = crate::array::js_array_alloc(values.len() as u32);
    for handle in &values {
        out = crate::array::js_array_push_f64(out, handle.get_nanbox_f64());
    }
    out
}

pub(super) fn call_listener_args(stream: f64, listener: f64, args: &[f64]) -> f64 {
    if !is_callable_value(listener) {
        return f64::from_bits(super::TAG_UNDEFINED);
    }
    unsafe {
        crate::closure::native_call_value_this(
            listener,
            crate::closure::JsThis::from_f64(stream),
            args.as_ptr(),
            args.len(),
        )
    }
}

/// node's `emitUnhandledRejectionOrErr`: a captured listener rejection goes to
/// the emitter's `[Symbol.for('nodejs.rejection')](err, type, ...args)` when
/// it has one, else to `emit('error', err)` with capture switched off for that
/// emit. Captures `[emitter, type, argsArray]`.
pub(super) extern "C" fn ns_capture_rejection(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    reason: f64,
) -> f64 {
    if closure.is_null() {
        return undefined_value();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 0));
    let event = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 1));
    let args = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 2));
    let reason = scope.root_nanbox_f64(reason);
    let hook_key = unsafe { crate::symbol::js_symbol_for(name_key(b"nodejs.rejection")) };
    let hook = scope.root_nanbox_f64(get_key(stream.get_nanbox_f64(), hook_key));
    if is_callable_value(hook.get_nanbox_f64()) {
        let mut call_args = vec![reason.get_nanbox_f64(), event.get_nanbox_f64()];
        if is_array_value(args.get_nanbox_f64()) {
            let arr = super::raw_ptr_from_value(args.get_nanbox_f64())
                as *const crate::array::ArrayHeader;
            for i in 0..crate::array::js_array_length(arr) {
                call_args.push(crate::array::js_array_get_f64(arr, i));
            }
        }
        unsafe {
            crate::closure::native_call_value_this(
                hook.get_nanbox_f64(),
                crate::closure::JsThis::from_f64(stream.get_nanbox_f64()),
                call_args.as_ptr(),
                call_args.len(),
            );
        }
        return undefined_value();
    }
    set_capture_rejections(stream.get_nanbox_f64(), false);
    let error = name_key(b"error");
    emit_via_method(stream.get_nanbox_f64(), &[error, reason.get_nanbox_f64()]);
    set_capture_rejections(stream.get_nanbox_f64(), true);
    undefined_value()
}

fn set_capture_rejections(stream: f64, enabled: bool) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    let key = super::hidden_capture_rejections_key();
    super::set_hidden_value(stream.get_nanbox_f64(), key, bool_value(enabled));
}

fn bool_value(value: bool) -> f64 {
    f64::from_bits(if value {
        super::TAG_TRUE
    } else {
        super::TAG_FALSE
    })
}

/// node's `EventEmitter.init(opts)` capture step: a truthy
/// `opts.captureRejections` must be a boolean, and turns capture on.
pub(crate) fn init_event_emitter_capture(target: f64, options: f64) {
    if !is_object_value(options) || !is_object_value(target) {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let value = get_named(options, b"captureRejections");
    if crate::value::js_is_truthy(value) == 0 {
        return;
    }
    if value.to_bits() != super::TAG_TRUE {
        let message = format!(
            "The \"options.captureRejections\" property must be of type boolean. Received {}",
            crate::fs::validate::describe_received(value)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    set_capture_rejections(target.get_nanbox_f64(), true);
}

fn capture_rejections_enabled(stream: f64) -> bool {
    super::has_truthy_hidden(stream, super::hidden_capture_rejections_key())
}

/// Mark an async listener's returned promise handled when its rejection is to
/// be swallowed (Node's Readable `data` path), so it is not reported as an
/// unhandled rejection at program end (#1545).
fn swallow_listener_rejection(result: f64) {
    if crate::promise::js_value_is_promise(result) == 0 {
        return;
    }
    let promise = crate::value::js_nanbox_get_pointer(result) as *mut crate::promise::Promise;
    if !promise.is_null() {
        crate::promise::mark_rejection_handled(promise);
    }
}

/// node's `addCatch`: route a listener's rejected promise to
/// [`ns_capture_rejection`] with the emit's type and arguments.
fn capture_listener_rejection(stream: f64, event: f64, args: &[f64], result: f64) {
    if crate::promise::js_value_is_promise(result) == 0 {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    let event = scope.root_nanbox_f64(event);
    let result = scope.root_nanbox_f64(result);
    let arg_handles = scope.root_nanbox_f64_slice(args);
    let mut arr = crate::array::js_array_alloc(arg_handles.len() as u32);
    for handle in &arg_handles {
        arr = crate::array::js_array_push_f64(arr, handle.get_nanbox_f64());
    }
    let arr = scope.root_nanbox_f64(super::box_pointer(arr as *const u8));
    let on_rejected = js_closure_alloc(
        crate::fn_info!(ns_capture_rejection, 1; with_declared(1)),
        3,
    );
    js_closure_set_capture_f64(on_rejected, 0, stream.get_nanbox_f64());
    js_closure_set_capture_f64(on_rejected, 1, event.get_nanbox_f64());
    js_closure_set_capture_f64(on_rejected, 2, arr.get_nanbox_f64());
    let promise = crate::value::js_nanbox_get_pointer(result.get_nanbox_f64())
        as *mut crate::promise::Promise;
    if promise.is_null() {
        return;
    }
    crate::promise::js_promise_then(promise, std::ptr::null(), on_rejected);
}

pub(super) fn emit_stream_event_from_array(
    stream: f64,
    event: f64,
    args_arr: *const crate::array::ArrayHeader,
) -> f64 {
    let len = if args_arr.is_null() {
        0
    } else {
        crate::array::js_array_length(args_arr)
    };
    let mut args = Vec::with_capacity(len as usize);
    for i in 0..len {
        args.push(crate::array::js_array_get_f64(args_arr, i));
    }
    emit_stream_event(stream, event, &args)
}

/// #9493: whether `stream` has a listener for `event` in THIS registry. The
/// fs-stream bridge asks before forwarding an `'error'` its own registry has
/// already delivered, so the unhandled-error throw below fires only when
/// neither registry had a listener.
pub(super) fn has_stream_listeners(stream: f64, event: f64) -> bool {
    stream_listener_count_for_event(stream, event) > 0
}

/// The arguments of one `emit`, rooted for its dispatch window. Up to
/// [`INLINE_ARGS`] are held without a heap allocation.
const INLINE_ARGS: usize = 4;

enum RootedArgs<'scope> {
    Inline(
        [Option<crate::gc::RuntimeHandle<'scope>>; INLINE_ARGS],
        usize,
    ),
    Heap(Vec<crate::gc::RuntimeHandle<'scope>>),
}

impl<'scope> RootedArgs<'scope> {
    fn new(scope: &'scope crate::gc::RuntimeHandleScope, args: &[f64]) -> Self {
        if args.len() > INLINE_ARGS {
            return RootedArgs::Heap(scope.root_nanbox_f64_slice(args));
        }
        let mut handles = [None, None, None, None];
        for (handle, arg) in handles.iter_mut().zip(args) {
            *handle = Some(scope.root_nanbox_f64(*arg));
        }
        RootedArgs::Inline(handles, args.len())
    }

    /// The arguments as they are now (a collection may have moved them).
    fn with_live<R>(&self, f: impl FnOnce(&[f64]) -> R) -> R {
        match self {
            RootedArgs::Inline(handles, len) => {
                let mut live = [0f64; INLINE_ARGS];
                for (slot, handle) in live.iter_mut().zip(handles.iter().flatten()) {
                    *slot = handle.get_nanbox_f64();
                }
                f(&live[..*len])
            }
            RootedArgs::Heap(handles) => f(
                &crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(handles),
            ),
        }
    }
}

/// node's `emit(type, ...args)`.
pub(super) fn emit_stream_event(stream: f64, event: f64, args: &[f64]) -> f64 {
    // #10600: a listener can allocate enough to trigger a moving collection,
    // so the receiver, the event, the arguments and the listener snapshot are
    // all rooted for the whole dispatch window and re-read before each call.
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream_h = scope.root_nanbox_f64(stream);
    let event_h = scope.root_nanbox_f64(event);
    let arg_handles = RootedArgs::new(&scope, args);

    let is_error = super::string_value_eq(event, b"error");
    if !is_error {
        // The shapes' answer for `this._events[type]`: no listener, or one.
        if let Some(handler) = listeners_of_fast(stream, event) {
            if is_undefined(handler) {
                return f64::from_bits(super::TAG_FALSE);
            }
            if is_callable_value(handler) {
                let handler = scope.root_nanbox_f64(handler);
                dispatch_listener(&stream_h, &event_h, &arg_handles, &handler, false);
                return f64::from_bits(super::TAG_TRUE);
            }
        }
    }
    if is_error {
        if let Some(first) = args.first() {
            let first = scope.root_nanbox_f64(*first);
            let key = super::hidden_error_key();
            super::set_hidden_value(stream_h.get_nanbox_f64(), key, first.get_nanbox_f64());
            super::refresh_readable_aborted_flag(stream_h.get_nanbox_f64());
        }
    }
    let mut events = events_of(stream_h.get_nanbox_f64());
    let mut unhandled_error = is_error;
    match events {
        Some(found) => {
            if is_error {
                let found = scope.root_nanbox_f64(found);
                let monitor = error_monitor_event();
                if !is_undefined(get_key(found.get_nanbox_f64(), monitor)) {
                    let mut monitor_args = Vec::with_capacity(args.len() + 1);
                    monitor_args.push(monitor);
                    arg_handles.with_live(|live| monitor_args.extend_from_slice(live));
                    emit_via_method(stream_h.get_nanbox_f64(), &monitor_args);
                }
                unhandled_error = is_undefined(get_event_named(found.get_nanbox_f64(), b"error"));
                // A monitor listener may have replaced `_events`.
                events = events_of(stream_h.get_nanbox_f64());
            }
        }
        None if !is_error => return f64::from_bits(super::TAG_FALSE),
        None => {}
    }
    if unhandled_error {
        let live = arg_handles.with_live(<[f64]>::to_vec);
        if emit_unhandled_error_to_domain(stream_h.get_nanbox_f64(), live.first().copied()) {
            return f64::from_bits(super::TAG_FALSE);
        }
        crate::os::os_process_emitter::throw_unhandled_error_event(&live);
    }

    let Some(events) = events else {
        return f64::from_bits(super::TAG_FALSE);
    };
    let handler = get_key(events, event_h.get_nanbox_f64());
    if is_undefined(handler) {
        return f64::from_bits(super::TAG_FALSE);
    }
    if is_callable_value(handler) {
        // One listener: nothing to snapshot.
        let handler = scope.root_nanbox_f64(handler);
        dispatch_listener(&stream_h, &event_h, &arg_handles, &handler, is_error);
        return f64::from_bits(super::TAG_TRUE);
    }
    // node clones the array before dispatch, so listeners added or removed
    // by a listener take effect from the next emit.
    let listener_values: Vec<f64> = if is_array_value(handler) {
        let arr = super::raw_ptr_from_value(handler) as *const crate::array::ArrayHeader;
        (0..crate::array::js_array_length(arr))
            .map(|i| crate::array::js_array_get_f64(arr, i))
            .collect()
    } else {
        Vec::new()
    };
    let listener_handles = scope.root_nanbox_f64_slice(&listener_values);
    for handle in &listener_handles {
        dispatch_listener(&stream_h, &event_h, &arg_handles, handle, is_error);
    }
    f64::from_bits(super::TAG_TRUE)
}

/// One listener call of an `emit`, and node's `addCatch` for a listener that
/// returned a promise.
fn dispatch_listener(
    stream_h: &crate::gc::RuntimeHandle<'_>,
    event_h: &crate::gc::RuntimeHandle<'_>,
    arg_handles: &RootedArgs<'_>,
    listener: &crate::gc::RuntimeHandle<'_>,
    is_error: bool,
) {
    let result = arg_handles.with_live(|live| {
        call_listener_args(stream_h.get_nanbox_f64(), listener.get_nanbox_f64(), live)
    });
    if crate::promise::js_value_is_promise(result) == 0 {
        return;
    }
    // Node's Readable data delivery path does not route async `data` listener
    // rejections through captureRejections; custom EventEmitter-style events do.
    // As in node's `addCatch`, the capture flag is consulted only for a
    // listener that returned a promise.
    let is_data = super::string_value_eq(event_h.get_nanbox_f64(), b"data");
    if !is_error && !is_data && capture_rejections_enabled(stream_h.get_nanbox_f64()) {
        arg_handles.with_live(|live| {
            capture_listener_rejection(
                stream_h.get_nanbox_f64(),
                event_h.get_nanbox_f64(),
                live,
                result,
            )
        });
    } else if is_data {
        // Node's Readable swallows a rejection returned by an async `data`
        // listener — it is neither captured to `error` nor surfaced as an
        // unhandled rejection. Mark it handled so it stays silent (#1545).
        swallow_listener_rejection(result);
    }
}

/// node's `domain` module: an emitter bound to a domain (`domain.add(ee)`, an
/// own `domain` property) hands an unhandled `'error'` to that domain instead
/// of throwing: `er.domainEmitter = this`, `er.domain = domain`,
/// `er.domainThrown = false`, then `domain.emit('error', er)`.
fn emit_unhandled_error_to_domain(target: f64, error: Option<f64>) -> bool {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let domain = get_named(target.get_nanbox_f64(), b"domain");
    let domain_value = JSValue::from_bits(domain.to_bits());
    if domain_value.is_undefined() || domain_value.is_null() || !domain_value.is_pointer() {
        return false;
    }
    let domain = scope.root_nanbox_f64(domain);
    let error = scope.root_nanbox_f64(error.unwrap_or_else(undefined_value));
    // `typeof er === 'object'`: an Error or any other object takes the notes.
    if JSValue::from_bits(error.get_nanbox_u64()).is_pointer()
        && unsafe { crate::symbol::js_is_symbol(error.get_nanbox_f64()) } == 0
    {
        for name in [&b"domainEmitter"[..], b"domain", b"domainThrown"] {
            let key = scope.root_nanbox_f64(name_key(name));
            // Read after the key's allocation: the handles track any move.
            let value = match name {
                b"domainEmitter" => target.get_nanbox_f64(),
                b"domain" => domain.get_nanbox_f64(),
                _ => bool_value(false),
            };
            unsafe {
                crate::object::js_object_set_property_key(
                    error.get_nanbox_f64(),
                    key.get_nanbox_f64(),
                    value,
                );
            }
        }
    }
    let args = [name_key(b"error"), error.get_nanbox_f64()];
    let method = b"emit";
    unsafe {
        crate::object::js_native_call_method(
            domain.get_nanbox_f64(),
            method.as_ptr() as *const i8,
            method.len(),
            args.as_ptr(),
            args.len(),
        );
    }
    true
}

fn error_monitor_event() -> f64 {
    unsafe { crate::symbol::js_symbol_for(super::literal_string_value(b"events.errorMonitor")) }
}
