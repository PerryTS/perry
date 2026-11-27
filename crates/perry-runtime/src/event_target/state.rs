//! Object-owned symbol properties; public symbols mirror Node, private keys
//! are excluded from reflection. No native-state record or backing object.
use crate::object::ObjectHeader;
use std::cell::RefCell;
pub(crate) const TYPE: u32 = 0;
pub(crate) const TARGET: u32 = 1;
pub(crate) const BUBBLES: u32 = 3;
pub(crate) const CANCELABLE: u32 = 4;
pub(crate) const DEFAULT_PREVENTED: u32 = 5;
pub(crate) const TIMESTAMP: u32 = 6;
pub(crate) const COMPOSED: u32 = 7;
pub(crate) const TRUSTED: u32 = 9;
pub(crate) const STOPPED: u32 = 10;
pub(crate) const IMMEDIATE_STOPPED: u32 = 11;
pub(crate) const DETAIL: u32 = 12;
pub(crate) const LISTENERS: u32 = 13;
pub(crate) const MAX_LISTENERS: u32 = 14;
pub(crate) const WARNED: u32 = 15;
pub(crate) const HANDLERS: u32 = 16;
pub(crate) const SIGNAL_ABORTED: u32 = 17;
pub(crate) const SIGNAL_REASON: u32 = 18;
pub(crate) const COMPOSITE: u32 = 20;
pub(crate) const CONTROLLER_SIGNAL: u32 = 21;
pub(crate) const DOM_NAME: u32 = 22;
pub(crate) const DOM_MESSAGE: u32 = 23;
pub(crate) const DISPATCHED: u32 = 24;
pub(crate) const PASSIVE: u32 = 25;
pub(crate) const MESSAGE_DATA: u32 = 26;
pub(crate) const MESSAGE_ORIGIN: u32 = 27;
pub(crate) const MESSAGE_ID: u32 = 28;
pub(crate) const MESSAGE_SOURCE: u32 = 29;
pub(crate) const MESSAGE_PORTS: u32 = 30;
const KEYS: &[(&str, bool)] = &[
    ("type", false),
    ("kTarget", false),
    ("currentTarget", true),
    ("bubbles", true),
    ("cancelable", true),
    ("defaultPrevented", true),
    ("timestamp", true),
    ("composed", true),
    ("eventPhase", true),
    ("isTrusted", true),
    ("propagationStopped", true),
    ("kStop", false),
    ("detail", false),
    ("kEvents", false),
    ("events.maxEventTargetListeners", false),
    ("events.maxEventTargetListenersWarned", false),
    ("kHandlers", false),
    ("kAborted", false),
    ("kReason", false),
    ("onabort", true),
    ("kComposite", false),
    ("signal", true),
    ("name", true),
    ("message", true),
    ("kIsBeingDispatched", false),
    ("kInPassiveListener", false),
    ("data", true),
    ("origin", true),
    ("lastEventId", true),
    ("source", true),
    ("ports", true),
];
crate::perry_thread_local! {
    static SYMBOLS: RefCell<[u64; KEYS.len()]> = const { RefCell::new([0; KEYS.len()]) };
}

pub(crate) fn scan_roots(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    SYMBOLS.with(|symbols| {
        for bits in symbols.borrow_mut().iter_mut() {
            if *bits != 0 {
                visitor.visit_nanbox_u64_slot(bits);
            }
        }
    });
}

fn symbol(index: u32) -> u64 {
    let cached = SYMBOLS.with(|symbols| symbols.borrow()[index as usize]);
    if cached != 0 {
        return cached;
    }
    let desc = KEYS[index as usize].0.as_bytes();
    let desc = crate::string::js_string_from_bytes(desc.as_ptr(), desc.len() as u32);
    let value =
        unsafe { crate::symbol::js_symbol_new(crate::value::js_nanbox_string(desc as i64)) };
    SYMBOLS.with(|symbols| {
        let mut symbols = symbols.borrow_mut();
        symbols[index as usize] = value.to_bits();
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
    });
    value.to_bits()
}

/// Is `owner` an object one of this module's families births: an ordinary
/// GC object (its attributes live in its own key list) of a family class id?
/// Those answer their own state keys from their layout ([`get_slot`],
/// [`set_slot`], [`has`]); every other owner (a program subclass instance,
/// a borrowed-method receiver, a DOMException error cell) takes the generic
/// property paths.
#[inline]
fn family_owner(owner: *mut ObjectHeader) -> bool {
    use crate::native_class_ids as ids;
    unsafe {
        crate::value::addr_class::try_read_tracked_gc_header(owner as usize).is_some_and(|h| {
            let h = h.as_ref();
            h.obj_type == crate::gc::GC_TYPE_OBJECT && h.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
        }) && matches!(
            (*owner).class_id,
            ids::ABORT_SIGNAL | ids::ABORT_CONTROLLER | ids::EVENT_TARGET | ids::EVENT | ids::CUSTOM_EVENT
        )
    }
}

pub(crate) fn get_slot(owner: *mut ObjectHeader, index: u32) -> f64 {
    let key = SYMBOLS.with(|symbols| symbols.borrow()[index as usize]);
    if key == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // The own-property step of [[Get]], answered from the owner's layout: an
    // own data property is its slot. An absent key (the chain), an accessor
    // or any other owner takes the generic path below.
    if family_owner(owner) {
        if let Some(bits) = unsafe {
            crate::object::shaped_symbols::own_data_value(
                owner,
                (key & crate::value::POINTER_MASK) as usize,
            )
        } {
            return f64::from_bits(bits);
        }
    }
    if !KEYS[index as usize].1 {
        return unsafe {
            crate::symbol::js_object_get_symbol_property(
                crate::value::js_nanbox_pointer(owner as i64),
                f64::from_bits(key),
            )
        };
    }
    require_private(owner, index);
    let bits = unsafe {
        crate::object::shaped_symbols::get(
            owner as usize,
            (key & crate::value::POINTER_MASK) as usize,
        )
    };
    f64::from_bits(bits.unwrap_or(crate::value::TAG_UNDEFINED))
}

pub(crate) fn set_slot(owner: *mut ObjectHeader, index: u32, value: f64) {
    // OrdinarySet's own writable data property step, answered from the
    // owner's layout: one slot store. Nothing allocates before it.
    let present = SYMBOLS.with(|symbols| symbols.borrow()[index as usize]);
    if present != 0
        && family_owner(owner)
        && unsafe {
            crate::object::shaped_symbols::store_own_data(
                owner,
                (present & crate::value::POINTER_MASK) as usize,
                value.to_bits(),
                KEYS[index as usize].1,
            )
        }
    {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_raw_mut_ptr(owner);
    let value = scope.root_nanbox_f64(value);
    let key = symbol(index);
    let entry = if KEYS[index as usize].1 {
        crate::object::shaped_symbols::PRIVATE_ENTRY
            | crate::object::key_attrs::ENTRY_NON_ENUMERABLE
    } else {
        0
    };
    unsafe {
        if !KEYS[index as usize].1 {
            let receiver = owner.with_const_ptr::<ObjectHeader, _>(|ptr| {
                crate::value::js_nanbox_pointer(ptr as i64)
            });
            let result = crate::proxy::js_reflect_set(
                receiver,
                f64::from_bits(key),
                value.get_nanbox_f64(),
                receiver,
            );
            if crate::value::js_is_truthy(result) == 0 {
                let message = b"Cannot update event state on this receiver";
                let message =
                    crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
                crate::exception::throw_core(crate::error::CoreError::type_error(message));
            }
            return;
        }
        assert!(owner.with_mut_ptr::<ObjectHeader, _>(
            |ptr| crate::object::shaped_symbols::define(
                ptr as usize,
                (key & crate::value::POINTER_MASK) as usize,
                value.get_nanbox_u64(),
                entry
            )
        ));
    }
}
pub(super) use get_slot as get;
pub(super) use set_slot as set;
pub(crate) fn link(owner: *mut ObjectHeader, name: &str) -> *mut ObjectHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_raw_mut_ptr(owner);
    let (_, result) = owner.across_mut(|| {
        let proto = scope.root_nanbox_f64(crate::object::builtin_prototype_value(name));
        owner.with_mut_ptr::<ObjectHeader, _>(|ptr| {
            crate::object::prototype_chain::object_link_class_default_prototype(
                ptr as usize,
                proto.get_nanbox_f64().to_bits(),
            );
        });
    });
    result
}

pub(crate) fn has(owner: *mut ObjectHeader, index: u32) -> bool {
    let key = SYMBOLS.with(|symbols| symbols.borrow()[index as usize]);
    if key != 0 && family_owner(owner) {
        if unsafe {
            crate::object::shaped_symbols::own_entry(
                owner,
                (key & crate::value::POINTER_MASK) as usize,
            )
        }
        .is_some()
        {
            return true;
        }
    }
    key != 0
        && unsafe {
            crate::object::shaped_symbols::entry(
                owner as usize,
                (key & crate::value::POINTER_MASK) as usize,
            )
            .is_some()
        }
}

pub(crate) fn initialize_target(owner: *mut ObjectHeader, signal: bool) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_raw_mut_ptr(owner);
    for (index, value) in [
        (LISTENERS, crate::value::TAG_UNDEFINED),
        (
            MAX_LISTENERS,
            (if signal { 0.0f64 } else { 10.0f64 }).to_bits(),
        ),
        (WARNED, crate::value::TAG_FALSE),
        (HANDLERS, crate::value::TAG_UNDEFINED),
    ] {
        owner.with_mut_ptr(|ptr| set_slot(ptr, index, f64::from_bits(value)));
    }
}

/// A family's birth layout: its instances are ordinary objects of
/// `class_id` on the realm's `%name.prototype%`, whose own keys are the state
/// keys of `slots` in that order, each holding its pointer-free birth value.
/// The list is the family's layout: the first birth in an agent defines the
/// keys in this order, every later one is born in the shape that reached.
pub(crate) struct Birth {
    class_id: u32,
    name: &'static str,
    identity: crate::object::BuiltinConstructorIdentity,
    slots: &'static [(u32, u64)],
}

const TARGET_SLOTS: [(u32, u64); 4] = [
    (LISTENERS, crate::value::TAG_UNDEFINED),
    (MAX_LISTENERS, 0x4024_0000_0000_0000), // 10.0
    (WARNED, crate::value::TAG_FALSE),
    (HANDLERS, crate::value::TAG_UNDEFINED),
];
const SIGNAL_SLOTS: [(u32, u64); 7] = [
    (LISTENERS, crate::value::TAG_UNDEFINED),
    (MAX_LISTENERS, 0), // 0.0
    (WARNED, crate::value::TAG_FALSE),
    (HANDLERS, crate::value::TAG_UNDEFINED),
    (SIGNAL_ABORTED, crate::value::TAG_FALSE),
    (SIGNAL_REASON, crate::value::TAG_UNDEFINED),
    (COMPOSITE, crate::value::TAG_FALSE),
];

pub(crate) const EVENT_TARGET_BIRTH: Birth = Birth {
    class_id: crate::native_class_ids::EVENT_TARGET,
    name: "EventTarget",
    identity: crate::object::BuiltinConstructorIdentity::EventTarget,
    slots: &TARGET_SLOTS,
};
pub(crate) const ABORT_SIGNAL_BIRTH: Birth = Birth {
    class_id: crate::native_class_ids::ABORT_SIGNAL,
    name: "AbortSignal",
    identity: crate::object::BuiltinConstructorIdentity::AbortSignal,
    slots: &SIGNAL_SLOTS,
};
pub(crate) const ABORT_CONTROLLER_BIRTH: Birth = Birth {
    class_id: crate::native_class_ids::ABORT_CONTROLLER,
    name: "AbortController",
    identity: crate::object::BuiltinConstructorIdentity::AbortController,
    slots: &[(CONTROLLER_SIGNAL, crate::value::TAG_UNDEFINED)],
};

/// A fresh instance of `birth`'s family in its final layout. The prototype's
/// birth record (`shaped_symbols::born_on_record`) gives the shape: one
/// allocation and plain slot initializations. The first birth in an agent
/// (or one whose record no longer validates) links the prototype and defines
/// the keys in layout order, then records the shape it reached.
pub(crate) fn born(birth: &Birth) -> *mut ObjectHeader {
    let proto = crate::array::intrinsic_prototype_addr(birth.identity) as *mut ObjectHeader;
    if !proto.is_null() {
        let mut values = [0u64; 8];
        for (value, &(_, bits)) in values.iter_mut().zip(birth.slots) {
            *value = bits;
        }
        if let Some(obj) = unsafe {
            crate::object::shaped_symbols::born_on_record(
                birth.class_id,
                proto,
                &values[..birth.slots.len()],
            )
        } {
            return obj;
        }
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = crate::object::js_object_alloc(birth.class_id, birth.slots.len() as u32);
    let obj = scope.root_raw_mut_ptr(obj);
    obj.with_mut_ptr(|ptr| {
        link(ptr, birth.name);
    });
    for &(index, bits) in birth.slots {
        obj.with_mut_ptr(|ptr| set_slot(ptr, index, f64::from_bits(bits)));
    }
    let proto = crate::array::intrinsic_prototype_addr(birth.identity) as *mut ObjectHeader;
    obj.with_mut_ptr(|ptr| {
        if !proto.is_null() {
            unsafe { crate::object::shaped_symbols::record_birth(birth.class_id, proto, ptr) };
        }
        ptr
    })
}

/// Borrowed EventTarget methods can initialize an ordinary object without a
/// family class id. Its own symbol shape still identifies the native state at
/// a worker boundary, before ordinary-object serialization drops symbol keys.
pub(crate) fn transfer_family(owner: *mut ObjectHeader) -> Option<&'static str> {
    if has(owner, CONTROLLER_SIGNAL) {
        Some("AbortController")
    } else if has(owner, SIGNAL_ABORTED) {
        Some("AbortSignal")
    } else if has(owner, MESSAGE_DATA) {
        Some("MessageEvent")
    } else if has(owner, DETAIL) {
        Some("CustomEvent")
    } else if has(owner, CANCELABLE) {
        Some("Event")
    } else if has(owner, LISTENERS) {
        Some("EventTarget")
    } else {
        None
    }
}

/// Private fields are not forgeable by copying an exposed public symbol.
pub(crate) fn require_private(owner: *mut ObjectHeader, index: u32) {
    if !has(owner, index) {
        super::prototype::throw_receiver("Event");
    }
}
