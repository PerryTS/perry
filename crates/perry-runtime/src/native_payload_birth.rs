//! An instance born in its construction's final shape.
//!
//! Every instance an [`alloc_with_state`](super::alloc_with_state) call site
//! makes gets the same own keys in the same order with the same attributes:
//! the site's own data keys, the hidden JS-state key, then the family's own
//! accessors (node's per-instance getters such as `StatementSync`'s
//! `sourceSQL`). Its JS state object, a null-prototype object, likewise gets
//! the site's state keys. So both shapes are facts of the call site. The
//! site's first birth in an agent builds them key by key through the ordinary
//! define paths and records the two ShapeIds the objects reached in the site's
//! [`BirthMemo`]; every later birth allocates each object directly in its
//! recorded shape and fills the slots: no key-add transition, no by-name
//! store, no accessor install. Each record is re-validated on every use
//! (`shapes::shape_is_filled_birth` and its attributed form), and ShapeIds
//! are never reused, so a record that no longer names those facts simply
//! stops replaying.

use super::*;

/// One own accessor an instance is born with. The getter and setter are the
/// per-realm singleton closures of `get` / `set`.
#[derive(Clone, Copy)]
pub struct OwnAccessor {
    pub name: &'static str,
    pub get: *const crate::closure::JsFunctionInfo,
    pub set: Option<*const crate::closure::JsFunctionInfo>,
    pub enumerable: bool,
    pub configurable: bool,
}

/// A call site's recorded final shapes: the instance's ShapeId in the high
/// half, its JS state's in the low half (0 = none recorded). Per agent, like
/// ShapeIds. Declare one with [`birth_memo!`](crate::birth_memo).
pub type BirthMemo = std::thread::LocalKey<std::cell::Cell<u64>>;

/// Declare a [`BirthMemo`]: `birth_memo!(static STMT_BIRTH);`.
#[macro_export]
macro_rules! birth_memo {
    ($vis:vis static $name:ident) => {
        ::std::thread_local! {
            $vis static $name: ::std::cell::Cell<u64> = const { ::std::cell::Cell::new(0) };
        }
    };
}

/// The site's instance with its JS state, born in the recorded final shapes
/// when the memo still names them, else built key by key and recorded.
pub(super) fn alloc_born<T: 'static>(
    family: &'static NativePayloadFamily,
    payload: T,
    external_bytes: usize,
    own: &[(&[u8], f64)],
    state: &[(&[u8], f64)],
    accessors: &[OwnAccessor],
    memo: &'static BirthMemo,
) -> f64 {
    let proto = family_prototype(family);
    if proto.is_null() {
        return undefined();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let values: Vec<_> = own
        .iter()
        .chain(state)
        .map(|&(_, value)| scope.root_nanbox_f64(value))
        .collect();
    let (own_values, state_values) = values.split_at(own.len());
    let recorded = memo.with(std::cell::Cell::get);
    if recorded != 0 {
        if let Some(obj) =
            unsafe { born_final(family, recorded, own_values, state_values, accessors) }
        {
            let obj = scope.root_raw_mut_ptr(obj);
            attach_rooted(
                &obj,
                family,
                Some(payload),
                plain_vtable::<T>(),
                external_bytes,
            );
            return obj.with_mut_ptr::<ObjectHeader, _>(|obj| {
                crate::value::js_nanbox_pointer(obj as i64)
            });
        }
    }
    // The site's first birth (or a record that no longer replays): the
    // ordinary sequence, then record what it reached.
    // Sized to its keys, so the recorded shape's live slots are its keys.
    let state_obj = crate::object::js_object_alloc_null_proto(0, state.len() as u32);
    if state_obj.is_null() {
        return undefined();
    }
    let state_obj = scope.root_raw_mut_ptr(state_obj);
    for (&(key, _), value) in state.iter().zip(state_values) {
        set_own(&scope, &state_obj, key, value.get_nanbox_f64());
    }
    let state_value =
        state_obj.with_mut_ptr::<ObjectHeader, _>(|o| crate::value::js_nanbox_pointer(o as i64));
    let mut all: Vec<(&[u8], f64)> = own
        .iter()
        .zip(own_values)
        .map(|(&(key, _), value)| (key, value.get_nanbox_f64()))
        .collect();
    all.push((JS_STATE_KEY, state_value));
    let slots = all.len() + accessors.len();
    let value = scope.root_nanbox_f64(alloc_cell(
        family,
        Some(payload),
        plain_vtable::<T>(),
        external_bytes,
        &all,
        slots,
        proto,
    ));
    for accessor in accessors {
        define_own_accessor(
            value.get_nanbox_f64(),
            accessor.name,
            accessor.get,
            accessor.set,
            accessor.enumerable,
            accessor.configurable,
        );
    }
    let obj = crate::JSValue::from_bits(value.get_nanbox_u64()).as_pointer::<ObjectHeader>();
    let state_obj = state_obj.with_mut_ptr::<ObjectHeader, _>(|o| o);
    unsafe {
        let instance = crate::object::shapes::object_shape_stamp(obj);
        let state_shape = crate::object::shapes::object_shape_stamp(state_obj);
        // Every later birth validates the record before it replays it.
        memo.with(|m| m.set(u64::from(instance) << 32 | u64::from(state_shape)));
    }
    value.get_nanbox_f64()
}

/// The prototype identity a family instance's shape names: its class-default
/// link to the family prototype (`born_instance`'s birth record).
unsafe fn family_proto_id(proto: *mut ObjectHeader) -> Option<u64> {
    let meta = (*proto).meta;
    (!meta.is_null()).then(|| (*meta).proto_serial)
}

/// An unpublished null-prototype object, as `js_object_alloc_null_proto`
/// initializes one before publishing its birth shape.
unsafe fn unpublished_state(count: u32) -> *mut ObjectHeader {
    let obj = crate::object::object_alloc_unpublished(0, count);
    let gc = crate::gc::header_from_trusted_user_ptr(obj.cast()).cast_mut();
    (*gc)._reserved |= crate::gc::OBJ_FLAG_NULL_PROTO;
    obj
}

/// Stamp a fresh, unobserved object with `id`.
unsafe fn stamp(obj: *mut ObjectHeader, id: u32) {
    if crate::arena::pointer_in_nursery(obj as usize) {
        // GC_STORE_AUDIT(POINTER_FREE): a ShapeId, never a heap reference.
        (*obj).parent_class_id = id;
    } else {
        crate::object::shapes::stamp_object_shape_id_with_carrier_note(obj, id);
    }
}

/// The family's shared accessor pairs, one per `accessors` entry in order:
/// a runtime-only array in its prototype's JS state (per realm, traced and
/// moved with the prototype; JS never reaches it, so a pair stays reachable
/// from JS only through an accessor slot), so every instance a birth replays holds the same pairs, as
/// every instance holds the same getter and setter closures. `None` when the
/// prototype's pairs are another accessor list's (that site then makes its
/// own pairs).
unsafe fn shared_pairs<'s>(
    scope: &'s crate::gc::RuntimeHandleScope,
    proto: *mut ObjectHeader,
    accessors: &[OwnAccessor],
) -> Option<crate::gc::RuntimeHandle<'s>> {
    let held = proto_pairs(proto);
    if crate::JSValue::from_bits(held.to_bits()).is_pointer() {
        return pairs_match(held, accessors).then(|| scope.root_nanbox_f64(held));
    }
    let proto = scope.root_raw_mut_ptr(proto);
    let pairs = scope.root_raw_mut_ptr(crate::array::js_array_alloc(accessors.len() as u32));
    for accessor in accessors {
        let get = crate::closure::js_closure_alloc_singleton(accessor.get);
        let get = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(get as i64));
        let set = accessor.set.map_or(0, |info| {
            let set = crate::closure::js_closure_alloc_singleton(info);
            crate::value::js_nanbox_pointer(set as i64).to_bits()
        });
        let pair = crate::object::accessor_pair::pair_new(crate::object::accessor_pair::Accessor {
            get: get.get_nanbox_u64(),
            set,
            ..Default::default()
        });
        let next = crate::array::js_array_push(
            pairs.get_raw_mut_ptr(),
            crate::JSValue::from_bits(crate::value::js_nanbox_pointer(pair as i64).to_bits()),
        );
        pairs.set_raw_mut_ptr(next);
    }
    let mut state = proto
        .with_mut_ptr::<ObjectHeader, _>(|p| raw_field_memo(p, JS_STATE_KEY, &PROTO_STATE_MEMO));
    if !crate::JSValue::from_bits(state.to_bits()).is_pointer() {
        let fresh = crate::object::js_object_alloc_null_proto(0, 1);
        state = crate::value::js_nanbox_pointer(fresh as i64);
        set_state_field(scope, &proto, JS_STATE_KEY, state);
        state = proto.with_mut_ptr::<ObjectHeader, _>(|p| {
            raw_field_memo(p, JS_STATE_KEY, &PROTO_STATE_MEMO)
        });
    }
    let state = root_pointer::<ObjectHeader>(scope, state);
    let value = pairs.with_mut_ptr::<crate::array::ArrayHeader, _>(|a| {
        crate::value::js_nanbox_pointer(a as i64)
    });
    set_state_field(scope, &state, OWN_PAIRS_KEY, value);
    Some(scope.root_nanbox_f64(value))
}

const OWN_PAIRS_KEY: &[u8] = b"ownAccessorPairs";
crate::state_key_memo!(static PROTO_STATE_MEMO);
crate::state_key_memo!(static OWN_PAIRS_MEMO);

/// The pairs array a prototype's JS state holds (`undefined` when none).
unsafe fn proto_pairs(proto: *mut ObjectHeader) -> f64 {
    let state = raw_field_memo(proto, JS_STATE_KEY, &PROTO_STATE_MEMO);
    let js = crate::JSValue::from_bits(state.to_bits());
    if !js.is_pointer() {
        return undefined();
    }
    raw_field_memo(
        js.as_pointer::<ObjectHeader>() as *mut ObjectHeader,
        OWN_PAIRS_KEY,
        &OWN_PAIRS_MEMO,
    )
}

/// Do `pairs` hold exactly `accessors`' functions, in order?
unsafe fn pairs_match(pairs: f64, accessors: &[OwnAccessor]) -> bool {
    let arr = crate::JSValue::from_bits(pairs.to_bits()).as_pointer::<crate::array::ArrayHeader>();
    if (*arr).length as usize != accessors.len() || (*arr).capacity < (*arr).length {
        return false;
    }
    let info_of = |bits: u64| -> *const crate::closure::JsFunctionInfo {
        if bits == 0 {
            return std::ptr::null();
        }
        let closure = (bits & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
        (*closure).info
    };
    let words = crate::array::array_elements_ptr(arr);
    accessors.iter().enumerate().all(|(i, accessor)| {
        crate::object::accessor_pair::pair_of_value(*words.add(i)).is_some_and(|pair| {
            info_of(pair.get) == accessor.get
                && info_of(pair.set) == accessor.set.unwrap_or(std::ptr::null())
        })
    })
}

/// Do the recorded shape's lanes hold any value in slots `0..count` (no
/// typed lane), so a newborn's slots take plain initializing stores?
fn any_lanes(id: u32, count: usize) -> bool {
    let rep = crate::object::shapes::shape_rep_by_id(id);
    (0..count as u32).all(|slot| {
        crate::object::field_rep::slot_rep(rep, slot) == crate::object::field_rep::REP_ANY
    })
}

/// Initialize slot `i` of an unobserved newborn stamped with an all-`Any`
/// shape: the canonical store's string alias and barrier work; the shape's
/// rep is what traces the slot.
unsafe fn init_slot(obj: *mut ObjectHeader, i: usize, bits: u64) {
    let fields = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut u64;
    crate::gc::runtime_store_jsvalue_slot_layout_deferred(
        obj as usize,
        fields.add(i) as usize,
        i,
        bits,
    );
}

/// Both objects in the recorded shapes, or `None` (nothing reachable was
/// made) when a record no longer names them. Each object is stamped and
/// filled before anything else allocates.
unsafe fn born_final(
    family: &'static NativePayloadFamily,
    recorded: u64,
    own: &[crate::gc::RuntimeHandle<'_>],
    state: &[crate::gc::RuntimeHandle<'_>],
    accessors: &[OwnAccessor],
) -> Option<*mut ObjectHeader> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let (instance_id, state_id) = ((recorded >> 32) as u32, recorded as u32);
    let pairs = if accessors.is_empty() {
        None
    } else {
        Some(shared_pairs(&scope, family_prototype(family), accessors)?)
    };
    let state_obj = unpublished_state(state.len() as u32);
    let kind = crate::object::shapes::store_kind::receiver_ordinary_kind(state_obj);
    if !crate::object::shapes::shape_is_filled_birth(
        state_id,
        crate::object::shapes::PROTO_ID_NULL,
        state.len() as u32,
        kind,
    ) || !any_lanes(state_id, state.len())
    {
        return None;
    }
    stamp(state_obj, state_id);
    for (i, value) in state.iter().enumerate() {
        init_slot(state_obj, i, value.get_nanbox_u64());
    }
    let state_obj = scope.root_raw_mut_ptr(state_obj);
    let count = own.len() + 1 + accessors.len();
    let obj = crate::object::object_alloc_unpublished(family.class_id, count as u32);
    let proto_id = family_proto_id(family_prototype(family))?;
    let kind = crate::object::shapes::store_kind::receiver_ordinary_kind(obj);
    if !crate::object::shapes::shape_is_attributed_filled_birth(
        instance_id,
        proto_id,
        count as u32,
        kind,
    ) || !any_lanes(instance_id, count)
    {
        return None;
    }
    if !accessors.is_empty() {
        // The accessors were born in the recorded keys with their
        // attributes: the object's descriptor bit is initialized with them,
        // as a builtin install leaves it (no table entry, no process gate).
        let gc = crate::gc::header_from_trusted_user_ptr(obj.cast()).cast_mut();
        (*gc)._reserved |= crate::gc::OBJ_FLAG_HAS_DESCRIPTORS;
    }
    stamp(obj, instance_id);
    for (i, value) in own.iter().enumerate() {
        init_slot(obj, i, value.get_nanbox_u64());
    }
    let state_bits = state_obj
        .with_mut_ptr::<ObjectHeader, _>(|o| crate::value::js_nanbox_pointer(o as i64).to_bits());
    init_slot(obj, own.len(), state_bits);
    if let Some(pairs) = pairs {
        let arr = crate::JSValue::from_bits(pairs.get_nanbox_u64())
            .as_pointer::<crate::array::ArrayHeader>();
        let words = crate::array::array_elements_ptr(arr);
        for i in 0..accessors.len() {
            init_slot(obj, own.len() + 1 + i, *words.add(i));
        }
    }
    Some(obj)
}
