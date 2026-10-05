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
use crate::object::ObjectHeader;
use crate::value::JSValue;

/// The bound `events.<export>` constructor. The events attach is armed first:
/// minting the constructor before it would cache one without its statics
/// (`defaultMaxListeners`, `once`, ...) for the whole process.
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
    crate::object::js_nm_install_events();
    crate::object::bound_native_callable_export_value("events", export)
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

thread_local! {
    /// The site memo for a constructor's own `prototype` in its property
    /// bag (`closure::props`): the bag words on which it is an own plain
    /// data slot.
    static BAG_PROTOTYPE_SLOT: crate::object::method_site::own_slot_memo::OwnSlotMemo =
        const { crate::object::method_site::own_slot_memo::OwnSlotMemo::new() };
}

/// The constructor's own `prototype` read from its property bag, where a
/// function's own data properties live (`closure::props::bag_get`), through
/// the bag's shape. A function's `prototype` is non-configurable, so no
/// accessor or deletion can stand in front of the bag's value. `None` when
/// the bag's shape does not answer (the full read then runs).
fn own_prototype_by_bag(ctor: f64) -> Option<f64> {
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
        let slot = match BAG_PROTOTYPE_SLOT.with(|memo| memo.slot(bag)) {
            Some(slot) => slot,
            None => {
                let meta = (*bag).meta;
                if !meta.is_null() && (*meta).elements != 0 {
                    return None;
                }
                let id = (*bag).parent_class_id;
                if !crate::object::shapes::is_site_matchable_shape_id(id) {
                    return None;
                }
                let record = crate::object::shapes::shape_record_by_id(id)?;
                let pos = record.own_data_position_of_value(0, b"prototype")??;
                BAG_PROTOTYPE_SLOT.with(|memo| memo.prime(bag, pos));
                pos
            }
        };
        // A wide bag keeps the key in its spill buffer: the read follows the
        // shape's live inline bound either way.
        let live = crate::object::shapes::shape_record_by_id((*bag).parent_class_id)?
            .live_inline_slot_count();
        let bits = crate::object::object_field_at_with_live(bag, slot, live).bits();
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

/// node's instance-state keys in `EventEmitter.init`'s order, packed the way
/// perry-codegen packs an object literal's keys; [`EMITTER_STATE_SHAPE_ID`]
/// is the id codegen derives from them (`expr/object_literal.rs`), so an
/// emitter and a literal `{ _events, _eventsCount, _maxListeners }` share one
/// key list.
const EMITTER_STATE_KEYS: &[u8] = b"_events\0_eventsCount\0_maxListeners\0";
const EMITTER_STATE_SHAPE_ID: u32 = literal_shape_id(EMITTER_STATE_KEYS);

const fn literal_shape_id(packed: &[u8]) -> u32 {
    let mut id: u32 = 0x811c_9dc5;
    let mut i = 0;
    while i < packed.len() {
        id ^= packed[i] as u32;
        id = id.wrapping_mul(0x0100_0193);
        i += 1;
    }
    if id == 0 {
        1
    } else {
        id
    }
}

/// What `new EventEmitter()`'s site memo proved about one prototype ShapeId:
/// the three state keys are plain writable own data, and `_maxListeners`
/// reads inline slot `max_slot` (below `live`, the shape's inline bound).
#[derive(Clone, Copy)]
struct PlainPrototype {
    shape: u32,
    max_slot: u32,
    live: u32,
}

crate::perry_thread_local! {
    /// `new EventEmitter()`'s site memo: the prototype ShapeId it last proved
    /// (see [`PlainPrototype`]), so that `init`'s three `[[Set]]`s are three
    /// own-property creations. A ShapeId names one key list, one descriptor
    /// state and one `[[Prototype]]`, and every use compares it with the
    /// prototype's current one: an accessor, a descriptor or a deletion on the
    /// prototype re-stamps it and the full `init` runs instead.
    static PLAIN_PROTOTYPE: std::cell::Cell<Option<PlainPrototype>> =
        const { std::cell::Cell::new(None) };
}

/// The prototype's `_maxListeners` when `proto` holds the three state keys
/// as ordinary writable own data (consulting the memo first); `None` sends
/// construction down the full `init`.
fn plain_prototype_max(proto: f64) -> Option<f64> {
    let jsval = JSValue::from_bits(proto.to_bits());
    if !jsval.is_pointer() {
        return None;
    }
    let obj = jsval.as_pointer::<ObjectHeader>();
    // SAFETY: a live prototype object (rooted by the caller).
    let shape = unsafe { crate::object::shapes::object_shape_stamp(obj) };
    if shape == 0 {
        return None;
    }
    let memo = PLAIN_PROTOTYPE
        .with(|memo| memo.get())
        .filter(|memo| memo.shape == shape);
    let memo = match memo {
        Some(memo) => memo,
        None => {
            let plain = [&b"_events"[..], b"_eventsCount", b"_maxListeners"]
                .into_iter()
                .all(|name| own_plain_data(proto, name));
            if !plain {
                return None;
            }
            // SAFETY: as above; the descriptor and key list are the shape's.
            let memo = unsafe {
                let descriptor = crate::object::shapes::object_shape_descriptor(obj)?;
                let keys = descriptor.keys as usize as *const crate::array::ArrayHeader;
                if keys.is_null() {
                    return None;
                }
                let slot = crate::object::keys_find_slot_by_bytes_resolved(
                    keys,
                    descriptor.logical_key_count,
                    b"_maxListeners",
                )?;
                if slot >= descriptor.live_inline_slot_count {
                    return None;
                }
                PlainPrototype {
                    shape,
                    max_slot: slot,
                    live: descriptor.live_inline_slot_count,
                }
            };
            PLAIN_PROTOTYPE.with(|cell| cell.set(Some(memo)));
            memo
        }
    };
    // SAFETY: the memo's shape is the prototype's current one, which pins
    // `max_slot` as an inline data slot below `live`.
    let value = unsafe {
        crate::object::field_get_set::object_field_at_with_live(obj, memo.max_slot, memo.live)
    };
    Some(f64::from_bits(value.bits()))
}

/// `name` is an own data property of `obj` that a `[[Set]]` through a
/// derived object would shadow: no accessor and no non-writable attribute.
fn own_plain_data(obj: f64, name: &[u8]) -> bool {
    let jsval = JSValue::from_bits(obj.to_bits());
    let addr = jsval.as_pointer::<u8>() as usize;
    // SAFETY: the lookup validates the address before it reads anything.
    let found = unsafe { crate::object::native_get::try_data_lookup_bytes(jsval, name) };
    if !matches!(found, Some(Some(_))) {
        return false;
    }
    let Some(header) = (unsafe { crate::value::addr_class::try_read_gc_header(addr) }) else {
        return false;
    };
    if header._reserved & crate::gc::OBJ_FLAG_HAS_DESCRIPTORS == 0 {
        return true;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_nanbox_f64(obj);
    let key = f64::from_bits(
        JSValue::string_ptr(crate::string::intern_ascii_literal(name) as *mut _).bits(),
    );
    let addr = JSValue::from_bits(obj.get_nanbox_u64()).as_pointer::<u8>() as usize;
    // SAFETY: a live object (rooted above).
    unsafe { crate::object::own_descriptors_skip_key(addr, key) }
}

thread_local! {
    /// `new EventEmitter()`'s birth memo: the ShapeId the last full birth
    /// ended on (the state keys at slots 0..3, linked to the prototype). A
    /// later birth replays it when the record still names those facts for
    /// the prototype in hand (`shapes::shape_is_filled_birth`); ShapeIds are
    /// never reused, so a stale id simply stops replaying.
    static FINAL_SHAPE: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// An emitter born initialized: the state keys' shape, linked to `proto` as
/// `Object.create` links, and node's `init` values in its slots
/// (`this._maxListeners || undefined` reads the prototype's `inherited_max`).
fn born_emitter(proto: f64, inherited_max: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_nanbox_f64(proto);
    let max = scope.root_nanbox_f64(
        if inherited_max.to_bits() != crate::value::TAG_UNDEFINED
            && crate::value::js_is_truthy(inherited_max) != 0
        {
            inherited_max
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        },
    );
    let events = scope.root_nanbox_f64(crate::node_stream::new_events_object());
    let recorded = FINAL_SHAPE.with(std::cell::Cell::get);
    if recorded != 0 {
        // One allocation stamped with the recorded final shape, then each
        // slot through the store funnel; nothing allocates in between.
        if let Some(obj) =
            crate::object::object_alloc_filled_birth(recorded, proto.get_nanbox_u64(), 3)
        {
            let events = JSValue::from_bits(events.get_nanbox_u64());
            // SAFETY: a fresh object born with three inline slots, which
            // nothing has observed yet. A number (`_eventsCount = 0`, whose
            // bits are valid in an Any or an F64 lane) and an `undefined` in
            // an Any lane are written as they are; anything else goes
            // through the store funnel.
            unsafe {
                let fields = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut u64;
                let rep = crate::object::shapes::shape_rep_by_id(recorded);
                let any = |slot| {
                    crate::object::field_rep::slot_rep(rep, slot)
                        == crate::object::field_rep::REP_ANY
                };
                // This unpublished birth was validated against its final
                // shape. Slot 0 is Any: no lane generalization or numeric
                // prefix retirement is needed. Keep the canonical store's
                // alias and generational/incremental barrier work.
                debug_assert!(any(0));
                crate::gc::runtime_store_jsvalue_slot_layout_deferred(
                    obj as usize,
                    fields as usize,
                    0,
                    events.bits(),
                );
                // GC_STORE_AUDIT(INIT): non-pointer bits into an unobserved newborn.
                *fields.add(1) = 0f64.to_bits();
                let max = max.get_nanbox_u64();
                if max == crate::value::TAG_UNDEFINED && any(2) {
                    // GC_STORE_AUDIT(INIT): as above (the slot already holds it).
                    *fields.add(2) = max;
                } else {
                    crate::object::store_object_field_slot(obj, 2, max);
                }
            }
            return crate::value::js_nanbox_pointer(obj as i64);
        }
    }
    let instance = scope.root_raw_mut_ptr(crate::object::js_object_alloc_with_shape(
        EMITTER_STATE_SHAPE_ID,
        3,
        EMITTER_STATE_KEYS.as_ptr(),
        EMITTER_STATE_KEYS.len() as u32,
    ));
    instance.with_mut_ptr::<ObjectHeader, _>(|obj| {
        crate::object::prototype_chain::object_link_created_prototype(
            obj as usize,
            proto.get_nanbox_u64(),
        )
    });
    let events = JSValue::from_bits(events.get_nanbox_u64());
    instance.with_mut_ptr::<ObjectHeader, _>(|obj| {
        crate::object::js_object_set_field(obj, 0, events);
        crate::object::js_object_set_field(obj, 1, JSValue::from_bits(0f64.to_bits()));
        crate::object::js_object_set_field(obj, 2, JSValue::from_bits(max.get_nanbox_u64()));
    });
    instance.with_mut_ptr::<ObjectHeader, _>(|obj| {
        // SAFETY: the live instance just built.
        let id = unsafe { crate::object::shapes::object_shape_stamp(obj) };
        FINAL_SHAPE.with(|cell| cell.set(id));
        crate::value::js_nanbox_pointer(obj as i64)
    })
}

/// `new EventEmitter(opts)`.
#[no_mangle]
pub extern "C" fn js_event_emitter_object_new(options: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let options = scope.root_nanbox_f64(options);
    let proto = scope.root_nanbox_f64(event_emitter_prototype_value("EventEmitter"));
    let instance = if let Some(max) = plain_prototype_max(proto.get_nanbox_f64()) {
        born_emitter(proto.get_nanbox_f64(), max)
    } else {
        let instance =
            scope.root_nanbox_f64(crate::object::js_object_create(proto.get_nanbox_f64()));
        init_event_emitter_state(instance.get_nanbox_f64());
        instance.get_nanbox_f64()
    };
    // No capture option exists for the common no-options call. No allocation
    // intervenes between the returned newborn and this return, so it needs
    // no extra root just to invoke a capture initializer that would decline.
    if options.get_nanbox_u64() == crate::value::TAG_UNDEFINED {
        return instance;
    }
    let instance = scope.root_nanbox_f64(instance);
    init_event_emitter_capture(instance.get_nanbox_f64(), options.get_nanbox_f64());
    instance.get_nanbox_f64()
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

/// node: `validateString(options.name, 'options.name')` for an options object
/// (a string `options` is the name itself, and an absent name defaults to
/// `new.target.name`).
fn validate_async_resource_name(options: f64) {
    let obj = super::object_ptr_from_value(options);
    let name = match obj {
        Some(obj) => {
            let key = crate::string::intern_ascii_literal(b"name");
            crate::object::js_object_get_field_by_name_f64(obj as *const ObjectHeader, key)
        }
        None => f64::from_bits(crate::value::TAG_UNDEFINED),
    };
    let name_value = JSValue::from_bits(name.to_bits());
    if name_value.is_undefined() || name_value.is_any_string() {
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
