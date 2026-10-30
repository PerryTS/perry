//! `new EventEmitter(opts)` and `new EventEmitterAsyncResource(opts)` (#10508).
//!
//! An emitter is an ordinary object: `[[Prototype]]` the shared
//! `EventEmitter.prototype` (`install_event_emitter_prototype`), and node's
//! instance state (`_events`, `_eventsCount`, `_maxListeners`) as its own
//! properties. Every method is the one shared prototype closure acting on
//! its receiver, so a plain emitter and a subclass instance are the same kind
//! of value: listeners are traced JS values in `_events`, nothing about an
//! emitter lives in native memory, and the collector moves it like any other
//! object.
use super::*;
use crate::codegen_abi::LayoutWord;
use crate::object::ObjectHeader;
use crate::value::JSValue;

/// The export declaration owns its constructor and prototype decoration.
/// Internal instance births use the same declaration as named/member reads,
/// including when no whole events namespace has been requested.
pub fn event_emitter_constructor_value(export: &str) -> f64 {
    let minted = match export {
        "EventEmitter" => {
            crate::object::native_module::minted_native_callable_export("events\0EventEmitter")
        }
        "EventEmitterAsyncResource" => crate::object::native_module::minted_native_callable_export(
            "events\0EventEmitterAsyncResource",
        ),
        _ => None,
    };
    if let Some(ctor) = minted {
        return ctor;
    }
    match export {
        "EventEmitter" => crate::object::native_module::event_emitter_export_value(0),
        "EventEmitterAsyncResource" => {
            crate::object::native_module::event_emitter_async_resource_export_value(0)
        }
        _ => f64::from_bits(crate::value::TAG_UNDEFINED),
    }
}

/// `<export>.prototype`, resolved exactly the way a subclass's parent edge
/// resolves it (`reserved_native_parent_prototype_bits`), so a direct
/// instance and a subclass prototype reach the same object by identity.
pub fn event_emitter_prototype_value(export: &str) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let ctor = scope.root_nanbox_f64(event_emitter_constructor_value(export));
    if let Some(proto) = own_prototype_by_bag(ctor.get_nanbox_f64()) {
        return proto;
    }
    crate::object::js_function_prototype_value_for_read(ctor.get_nanbox_f64())
}

/// The constructor's own `prototype` read from its property bag, where a
/// function's own data properties live (`closure::props::bag_get`), through
/// the bag's shape. A function's `prototype` is non-configurable, so no
/// accessor or deletion can stand in front of the bag's value. `None` when
/// the bag's shape does not answer (the full read then runs).
pub(crate) fn own_prototype_by_bag(ctor: f64) -> Option<f64> {
    let value = JSValue::from_bits(ctor.to_bits());
    if !value.is_pointer() {
        return None;
    }
    let addr = value.as_pointer::<u8>() as usize;
    // SAFETY: the reader validates the address before it reads anything.
    let header = unsafe { crate::value::addr_class::try_read_gc_header(addr) }?;
    if header.obj_type != crate::gc::GC_TYPE_CLOSURE {
        return None;
    }
    // SAFETY: a live closure cell (its header says so).
    let bag = unsafe { (*(addr as *const crate::closure::ClosureHeader)).props };
    // SAFETY: as above, for the bag object.
    let bag_header = unsafe { crate::value::addr_class::try_read_gc_header(bag as usize) }?;
    if bag_header.obj_type != crate::gc::GC_TYPE_OBJECT
        || bag_header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    // SAFETY: a live ordinary object; nothing below allocates.
    unsafe {
        const SPILL: u32 = 1 << 31;
        let encoded = {
            let meta = (*bag).meta;
            if !meta.is_null() && (*meta).elements != 0 {
                return None;
            }
            let id = (*bag).shape_word();
            if !crate::object::shapes::is_site_matchable_layout_word(id) {
                return None;
            }
            let record = crate::object::shapes::shape_record_for_word(LayoutWord(id))?;
            let pos = record.own_data_position_of_value(0, b"prototype")??;
            pos | if pos >= record.live_inline_slot_count() {
                SPILL
            } else {
                0
            }
        };
        // The shape assigns the slot's canonical storage.
        let slot = (encoded & !SPILL) as usize;
        let bits = if encoded & SPILL == 0 {
            let fields = (bag as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
            *fields.add(slot)
        } else {
            let spill = (*(*bag).meta).spill as *const crate::array::ArrayHeader;
            *crate::array::array_elements_ptr(spill).add(slot)
        };
        (bits != crate::value::TAG_HOLE).then_some(f64::from_bits(bits))
    }
}

/// `Object.create(<export>.prototype)`: an ordinary object born on the
/// shared prototype, whose shape names that prototype.
fn alloc_emitter(export: &str) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_nanbox_f64(event_emitter_prototype_value(export));
    crate::object::js_object_create(proto.get_nanbox_f64())
}

/// node's `EventEmitter.init` on a fresh object whose prototype is `proto`,
/// born on `layout` (the emitter's, or the legacy Stream's) when `proto` is
/// the sealed prototype it names and its chain is unwritten
/// (`instance_birth::sealed_chain`): `_events` a fresh null-prototype
/// object, `_eventsCount` 0, and `_maxListeners ||= undefined` reading the
/// prototype's `undefined`. Any other prototype runs the ordinary `init` on
/// `Object.create(proto)`. The flag says whether the layout birth ran.
fn emitter_birth(
    proto: f64,
    layout: *const crate::object::layout_desc::PerryLayoutDesc,
) -> (f64, bool) {
    use crate::object::runtime_layouts::emitter_slot;
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_nanbox_f64(proto);
    // SAFETY: `layout` is a registered emitter layout.
    if !unsafe { super::instance_birth::sealed_chain(proto.get_nanbox_f64(), layout) } {
        let instance =
            scope.root_nanbox_f64(crate::object::js_object_create(proto.get_nanbox_f64()));
        init_event_emitter_state(instance.get_nanbox_f64());
        return (instance.get_nanbox_f64(), false);
    }
    let events = scope.root_nanbox_f64(crate::node_stream::new_events_object());
    // SAFETY: a registered descriptor; nothing observes the newborn until the
    // three stores below are done.
    // The three slots are the layout's whole key sequence, filled in its
    // order: each fill is the newest key, so in place.
    unsafe {
        let obj = crate::object::layout_desc::perry_alloc_layout(layout);
        let fields = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut u64;
        crate::gc::runtime_store_jsvalue_slot_layout_deferred(
            obj as usize,
            fields.add(emitter_slot::EVENTS as usize) as usize,
            emitter_slot::EVENTS as usize,
            events.get_nanbox_u64(),
        );
        // GC_STORE_AUDIT(INIT): non-pointer bits into an unobserved newborn.
        *fields.add(emitter_slot::EVENTS_COUNT as usize) = 0f64.to_bits();
        // GC_STORE_AUDIT(INIT): undefined into an unobserved newborn.
        *fields.add(emitter_slot::MAX_LISTENERS as usize) = crate::value::TAG_UNDEFINED;
        (box_pointer(obj.cast()), true)
    }
}

/// node's `EventEmitter(opts)` on a fresh object linked to `proto`: the
/// emitter birth, then the `captureRejections` option. On a layout birth the
/// sealed, unwritten `EventEmitter.prototype` holds its `kCapture` default
/// (false), so without options there is nothing to record.
pub(crate) fn emitter_new(
    proto: f64,
    options: f64,
    layout: *const crate::object::layout_desc::PerryLayoutDesc,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let options = scope.root_nanbox_f64(options);
    let (instance, sealed) = emitter_birth(proto, layout);
    if sealed && options.get_nanbox_u64() == crate::value::TAG_UNDEFINED {
        return instance;
    }
    let instance = scope.root_nanbox_f64(instance);
    crate::node_stream::event_emitter::init_newborn_emitter_capture(&instance, &options, None);
    instance.get_nanbox_f64()
}

/// `new EventEmitter(opts)`.
#[no_mangle]
pub extern "C" fn js_event_emitter_object_new(options: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let options = scope.root_nanbox_f64(options);
    let ctor = event_emitter_constructor_value("EventEmitter");
    let proto = own_prototype_by_bag(ctor)
        .unwrap_or_else(|| crate::object::js_function_prototype_value_for_read(ctor));
    emitter_new(
        proto,
        options.get_nanbox_f64(),
        &raw const crate::object::runtime_layouts::EVENT_EMITTER,
    )
}
/// `new EventEmitterAsyncResource(opts)`.
#[no_mangle]
pub extern "C" fn js_event_emitter_async_resource_object_new(options: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let options = scope.root_nanbox_f64(options);
    if !JSValue::from_bits(options.get_nanbox_u64()).is_any_string() {
        validate_async_resource_name(options.get_nanbox_f64());
    }
    let instance = alloc_emitter("EventEmitterAsyncResource");
    js_event_emitter_async_resource_subclass_init(instance, options.get_nanbox_f64())
}

/// node's constructor body for a DIRECT `new EventEmitterAsyncResource(opts)`:
/// `validateString(options?.name, 'options.name')`. A string `options` is the
/// name itself and never reaches here; for every other shape the property must
/// be a string, so a missing name (`undefined`, `{}`, `null`) throws too — the
/// `new.target.name` default applies only to subclasses, which enter through
/// `js_event_emitter_async_resource_subclass_init`.
fn validate_async_resource_name(options: f64) {
    let obj = super::object_ptr_from_value(options);
    let name = match obj {
        Some(obj) => unsafe { crate::runtime_state_key!(b"name").read_object(obj) },
        None => f64::from_bits(crate::value::TAG_UNDEFINED),
    };
    if JSValue::from_bits(name.to_bits()).is_any_string() {
        return;
    }
    let message = format!(
        "The \"options.name\" property must be of type string. Received {}",
        crate::fs::validate::describe_received(name)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

/// `EventEmitterAsyncResource(opts)` without `new`: a class constructor.
#[no_mangle]
pub extern "C" fn js_event_emitter_async_resource_call(_options: f64) -> f64 {
    let message = b"Class constructor EventEmitterAsyncResource cannot be invoked without 'new'";
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}
