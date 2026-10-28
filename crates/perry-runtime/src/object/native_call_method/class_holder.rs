//! A by-name method call on a class instance, answered by its prototype
//! chain's shapes (class-table retirement slice 3).
//!
//! A class's methods are own data properties of its prototype object (slice 1:
//! each slot holds a function object of the method's own body, a ConstFn lane
//! of the prototype's shape; slice 2: the class reaches that object through its
//! own function object). `recv.m(args)` is therefore `[[Get]](recv, "m")`
//! followed by a call, and the [[Get]] is a walk of SHAPES: the receiver's
//! shape lacks `m`, each prototype's shape lists its keys, and the first holder
//! whose shape lists `m` has it in the slot its shape names. Nothing here asks
//! the class registry which method a class id has, and nothing is cached by
//! class id or by name: a holder's key list is the only place a name is
//! compared, by atom identity first (a canonical key list holds its text's
//! atom, which is also what a compiled call site passes) and by bytes for a
//! list written before its atom existed.
//!
//! A shape fact changes with the object it describes: a method assigned,
//! deleted or redefined on a prototype, or a prototype relinked, restamps that
//! object, so the walk reads the current answer every time. Whatever the walk
//! cannot read from shapes alone (an accessor, a dictionary-mode or exotic
//! holder, an unlinked prototype identity) is handed back as [`ChainMethod::Get`]
//! for the caller's ordinary [[Get]] from that holder.

use super::*;

/// The name being called.
#[derive(Clone, Copy)]
pub(crate) struct MethodKey<'a> {
    pub(crate) bytes: &'a [u8],
}

impl<'a> MethodKey<'a> {
    #[inline]
    pub(crate) fn bytes(bytes: &'a [u8]) -> Self {
        MethodKey { bytes }
    }
}

/// What a class instance's prototype chain answers for a method name.
pub(crate) enum ChainMethod {
    /// A data property: its value, and the body its holder's shape names for
    /// the slot (a ConstFn lane: every carrier of that ShapeId holds a
    /// function object of that body there), if it names one.
    Data {
        value: u64,
        body: Option<&'static crate::closure::JsFunctionInfo>,
        /// The object whose shape lists the name.
        holder: *const ObjectHeader,
        /// The holder's inline slot that holds `value`, when it is one.
        #[allow(dead_code)] // the holder lane is part of the Data answer
        slot: Option<u32>,
    },
    /// `holder` (on the chain, every object before it lacks the name) needs
    /// an ordinary [[Get]] with the original receiver: an accessor, or an
    /// object whose shape alone does not answer.
    Get { holder: *const ObjectHeader },
    /// No object on the chain has the name.
    Absent,
}

/// Prototype chains are finite and acyclic; this bounds a walk over a
/// corrupt one.
const MAX_CHAIN_DEPTH: usize = 128;

/// Key lists at least this long are searched through the shape's shared key
/// index rather than scanned.
const INDEXED_SCAN_MIN_KEYS: u32 = 24;

/// The position of `key` among the first `count` keys of `keys`, scanned back
/// to front (#10595: the most-derived duplicate wins): atom identity, then
/// bytes. Allocation-free.
///
/// # Safety
/// `keys` is null or a live key list.
#[inline]
pub(crate) unsafe fn key_position(
    keys: *const ArrayHeader,
    count: u32,
    key: &MethodKey<'_>,
) -> Option<u32> {
    if keys.is_null() || count == 0 {
        return None;
    }
    if count >= INDEXED_SCAN_MIN_KEYS {
        return own_slot::find_method_slot(keys, count, key.bytes);
    }
    let (slots, len) = crate::object::keys_array_dense_slots_resolved(keys);
    if slots.is_null() {
        return crate::object::keys_find_slot_by_bytes(keys, count, key.bytes);
    }
    let n = (count as usize).min(len);
    let want = key.bytes;
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    for i in (0..n).rev() {
        let bits = (*slots.add(i)).to_bits();
        match bits >> 48 {
            0x7FFF => {
                let sp = (bits & crate::value::POINTER_MASK) as *const crate::StringHeader;
                if !sp.is_null()
                    && (*sp).byte_len as usize == want.len()
                    && std::slice::from_raw_parts(crate::string::string_data(sp), want.len())
                        == want
                {
                    return Some(i as u32);
                }
            }
            0x7FF9 => {
                if crate::string::js_string_key_bytes(crate::JSValue::from_bits(bits), &mut sso)
                    == Some(want)
                {
                    return Some(i as u32);
                }
            }
            _ => {}
        }
    }
    None
}

/// Is `addr` an ordinary object (no element storage the name could index,
/// no exotic read), so that its shape's key list plus its dictionary storage
/// (when it has any) are all its own string-keyed properties?
#[inline]
pub(crate) unsafe fn shape_answers(addr: usize) -> bool {
    if !crate::value::addr_class::is_above_handle_band(addr) {
        return false;
    }
    let Some(header) = crate::value::addr_class::try_read_gc_header(addr) else {
        return false;
    };
    // A function object is a GC_TYPE_CLOSURE cell, never GC_TYPE_OBJECT.
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || header._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0
    {
        return false;
    }
    let obj = addr as *const ObjectHeader;
    if (*obj).class_id == NATIVE_MODULE_CLASS_ID {
        return false;
    }
    let meta = (*obj).meta;
    meta.is_null()
        || ((*meta).elements == 0
            && (*meta).flags
                & (crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER
                    | crate::object::OBJECT_META_FLAG_NATIVE_ALIAS)
                == 0)
}

/// The [[Prototype]] of holder `obj` as its link records it: `Ok(Some)` an
/// object, `Ok(None)` null, `Err(())` a link the shape alone does not name.
#[inline]
pub(crate) unsafe fn next_holder(
    obj: *const ObjectHeader,
) -> Result<Option<*const ObjectHeader>, ()> {
    use crate::object::shapes::{PROTO_ID_DEFAULT, PROTO_ID_NULL};
    let word = crate::object::shapes::object_prototype_word(obj);
    if word != 0 {
        let v = crate::JSValue::from_bits(word);
        if v.is_pointer() {
            return Ok(Some(v.as_pointer::<ObjectHeader>() as *const ObjectHeader));
        }
        return if word == crate::value::TAG_NULL {
            Ok(None)
        } else {
            Err(())
        };
    }
    match crate::object::shapes::shape_proto_id(crate::object::shapes::object_shape_stamp(obj)) {
        Some(PROTO_ID_NULL) => Ok(None),
        Some(PROTO_ID_DEFAULT) => {
            if obj as usize == crate::array::object_prototype_addr_if_resolved() {
                // `%Object.prototype%` is immutable-prototype: null.
                return Ok(None);
            }
            let p = crate::array::object_prototype_addr_if_resolved();
            if p == 0 {
                Err(())
            } else {
                Ok(Some(p as *const ObjectHeader))
            }
        }
        _ => Err(()),
    }
}

/// Look `key` up on the chain that starts at `start` (a class instance's
/// prototype): the first holder whose shape lists it answers. Allocation-free,
/// never calls user code.
///
/// # Safety
/// `start` is a live object.
#[inline(never)]
pub(crate) unsafe fn chain_method(start: *const ObjectHeader, key: &MethodKey<'_>) -> ChainMethod {
    chain_method_admitting(start, key, |_| true)
}

/// [`chain_method`] that asks `admit` about each holder before reading its
/// shape: a refused holder answers [`ChainMethod::Get`] with itself, exactly
/// as a holder the shape cannot answer does. `admit` runs no user code.
///
/// # Safety
/// `start` is a live object.
#[inline(always)]
unsafe fn chain_method_admitting(
    start: *const ObjectHeader,
    key: &MethodKey<'_>,
    mut admit: impl FnMut(*const ObjectHeader) -> bool,
) -> ChainMethod {
    let mut holder = start;
    for _ in 0..MAX_CHAIN_DEPTH {
        if !admit(holder) || !shape_answers(holder as usize) {
            return ChainMethod::Get { holder };
        }
        // A family object with no overflow: its layout's index answers, and
        // a slot holding the hole is an absent key.
        let family_data = |family: Option<Option<(u32, u32)>>| {
            let Some(Some((_, slot))) = family else {
                return None;
            };
            let live = crate::object::object_live_slot_count(holder);
            let value =
                crate::object::field_get_set::object_field_at_with_live(holder, slot, live).bits();
            (value != crate::value::TAG_HOLE).then_some(ChainMethod::Data {
                value,
                body: None,
                holder,
                slot: (slot < live).then_some(slot),
            })
        };
        // A static family word answers from its layout, with no record read.
        let word = (*holder).parent_class_id;
        let static_family = crate::object::layout_desc::word_in_layout_index(word);
        let mut family = if static_family {
            crate::object::layout_desc::family_own_key(holder, key.bytes)
        } else {
            None
        };
        if let Some(data) = family_data(family) {
            return data;
        }
        let Some(record) = crate::object::shapes::object_shape_record(holder) else {
            return ChainMethod::Get { holder };
        };
        // A class evaluation's copy of a family layout: the record this walk
        // reads anyway names the family (`layout_desc::family_desc_of`).
        if !static_family && word & crate::object::layout_desc::LAYOUT_SHADOW_BIT == 0 {
            if let Some(d) = crate::object::layout_desc::evaluation_layout_desc(
                record.family_desc_index_plus_one(),
            ) {
                family = crate::object::layout_desc::desc_own_key(d, key.bytes, None);
                if let Some(data) = family_data(family) {
                    return data;
                }
            }
        }
        let keys = record.keys() as usize as *const ArrayHeader;
        // A layout key lives in its fixed slot, and the hole marker there is
        // its absence (§3.2); any other own key of a shadowed holder is in
        // its overflow.
        let shadowed = crate::object::overflow::shadowed(holder);
        let position = if keys.is_null() || family.is_some() {
            None
        } else {
            key_position(keys, record.logical_key_count(), key)
        };
        if shadowed && position.is_none() {
            let t = crate::object::overflow::table(holder);
            if !t.is_null() && crate::object::overflow::find_bytes(t, key.bytes).is_some() {
                return ChainMethod::Get { holder };
            }
        } else if let Some(pos) = position {
            // An ordinary object's key list says whether a key is an accessor
            // (its slot holds the pair); the descriptor tables hold nothing
            // more for it.
            if crate::object::key_attrs::key_is_accessor_at(keys, pos) {
                return ChainMethod::Get { holder };
            }
            let pos = crate::object::key_attrs::key_storage_slot(keys, pos);
            // Below the shape's live inline count the key IS that inline
            // slot; above it the general read finds the spill position.
            let inline = pos < record.live_inline_slot_count();
            let value = if inline {
                std::ptr::read(
                    (holder as *const u8)
                        .add(std::mem::size_of::<ObjectHeader>() + pos as usize * 8)
                        as *const u64,
                )
            } else {
                crate::object::js_object_get_field(holder, pos).bits()
            };
            if value == crate::value::TAG_HOLE {
                if !shadowed {
                    return ChainMethod::Get { holder };
                }
            } else {
                let body = if pos < crate::object::field_rep::REP_SLOTS
                    && record.special_constfn_mask() & (1 << pos) != 0
                {
                    record.constfn_info(pos).and_then(|info| {
                        (info as usize as *const crate::closure::JsFunctionInfo).as_ref()
                    })
                } else {
                    None
                };
                return ChainMethod::Data {
                    value,
                    body,
                    holder,
                    slot: inline.then_some(pos),
                };
            }
        }
        match next_holder(holder) {
            Ok(Some(next)) if next != holder => holder = next,
            Ok(Some(_)) | Err(()) => return ChainMethod::Get { holder },
            Ok(None) => return ChainMethod::Absent,
        }
    }
    ChainMethod::Get { holder: start }
}

/// The inherited read of `key` on an instance of a declared class, when the
/// chain's shapes decide it without the generic read: `Some(undefined)` when
/// neither the instance nor any holder up to `%Object.prototype%` lists the
/// name, `Some(value)` when `%Object.prototype%` supplies it as a data
/// property, `None` when anything else (a class member, an accessor, a link
/// the declaration does not name, a receiver the class surface does not
/// decide alone) needs the generic read.
///
/// The generic read of such an instance consults, after the own keys, the
/// class's accessors, its prototype registrations, a per-instance
/// `[[Prototype]]`, an evaluation's pinned heritage and finally
/// `%Object.prototype%`. Every one of them answers from a holder's shape or
/// from a link the shapes name, so a chain of pure compiled declared
/// prototypes (each linked to its declared parent's prototype, no registered
/// prototype object beside it) whose shapes all lack the name has nothing to
/// answer with. The walk is live: a method or accessor added to any holder, a
/// holder relinked, an instance given another prototype or an own property
/// changes the shape it reads, so no verdict is kept and nothing needs
/// retiring. A class whose prototype object is not built yet declines (the
/// generic read builds it).
///
/// # Safety
/// `receiver` is a pointer value.
#[inline]
pub(crate) unsafe fn class_instance_inherited_read(
    receiver: crate::JSValue,
    key: &[u8],
) -> Option<crate::JSValue> {
    // `constructor` and `__proto__` are answered by the class and the realm
    // (a prototype's back-edge, an accessor), `#x` and `@@x` by private
    // members and symbol-keyed members.
    if key.is_empty()
        || key[0] == b'#'
        || key == b"constructor"
        || key == b"__proto__"
        || key.starts_with(b"@@")
        || is_non_property_member_name(key)
    {
        return None;
    }
    let recv = super::class_receiver_fast_guard(f64::from_bits(receiver.bits()), key)?;
    let obj = recv.addr as *const ObjectHeader;
    if !recv.recorded_prototype.is_null()
        || crate::object::shapes::object_prototype_word(obj) != 0
        || !shape_answers(recv.addr)
    {
        return None;
    }
    // The class the receiver's shape names, a bare CLASS identity: no
    // per-instance prototype, no per-evaluation class.
    let named = shape_named_class(obj)?;
    if named == 0
        || named >= crate::object::class_value::FIRST_RUNTIME_CLASS_ID
        || crate::object::class_registry::instance_pinned_constructing_class(obj).is_some()
    {
        return None;
    }
    let start = crate::object::class_registry::class_decl_prototype_object(named);
    if start.is_null() {
        return None;
    }
    let object_prototype = crate::array::object_prototype_addr_if_resolved();
    if object_prototype == 0 {
        return None;
    }
    // `Some(class)`: the next holder must be that class's declared prototype;
    // `None`: the declared chain is over and the next must be
    // `%Object.prototype%`.
    let mut class = Some(named);
    let found = chain_method_admitting(start, &MethodKey::bytes(key), |holder| match class {
        Some(c) => {
            if c == 0
                || c >= crate::object::class_value::FIRST_RUNTIME_CLASS_ID
                || crate::object::class_registry::class_decl_prototype_object(c).cast_const()
                    != holder
                || !crate::object::class_registry::class_prototype_object(c).is_null()
                || crate::object::class_registry::prototype_objects::decl_prototype_relinked(
                    c,
                    holder.cast_mut(),
                )
            {
                return false;
            }
            class = match crate::object::class_registry::get_parent_class_id(c) {
                Some(parent) if parent != 0 && parent != c => Some(parent),
                _ => None,
            };
            true
        }
        None => holder as usize == object_prototype,
    });
    match found {
        ChainMethod::Absent => Some(crate::JSValue::undefined()),
        ChainMethod::Data { value, holder, .. } if holder as usize == object_prototype => {
            Some(crate::JSValue::from_bits(value))
        }
        _ => None,
    }
}

/// The class whose declared prototype `obj`'s shape names as its
/// [[Prototype]] (a bare CLASS identity, `shapes::class_proto_id`: the class's
/// generic origin for a specialization), or `None`. Only a declared class's
/// prototype is linked that way, and the class's function object keeps that
/// link for the agent's life (slice 2).
#[inline]
unsafe fn shape_named_class(obj: *const ObjectHeader) -> Option<u32> {
    use crate::object::shapes::{PROTO_ID_CLASS, PROTO_ID_MIXED};
    let pid =
        crate::object::shapes::shape_proto_id(crate::object::shapes::object_shape_stamp(obj))?;
    (PROTO_ID_CLASS..PROTO_ID_MIXED)
        .contains(&pid)
        .then_some(pid as u32)
}

/// The prototype a class instance's chain starts at when it already exists:
/// the receiver's recorded [[Prototype]], else the declared prototype its
/// shape names, read from the class's function object. Null when it is not
/// built yet, or the receiver's link is neither.
///
/// # Safety
/// `obj` is a live object.
#[inline]
pub(crate) unsafe fn class_instance_prototype(obj: *const ObjectHeader) -> *const ObjectHeader {
    let word = crate::object::shapes::object_prototype_word(obj);
    if word != 0 {
        return word_object(word);
    }
    match shape_named_class(obj) {
        Some(class_id) => word_object(crate::object::class_holder_word_selected(class_id)),
        None => std::ptr::null(),
    }
}

/// [`class_instance_prototype`] for a receiver that passed the class receiver
/// guard with no recorded [[Prototype]] other than its class's own
/// ([`super::ClassReceiver::recorded_prototype`] null). Such a receiver's
/// shape names its class's declared prototype (`shapes::class_proto_id`): the class's own link, or,
/// for a generic specialization (whose class keeps no link of its own), the
/// generic origin's, which the shape names.
///
/// # Safety
/// `obj` is a live object that passed [`class_receiver_guard`].
#[inline]
unsafe fn guarded_class_instance_prototype(obj: *const ObjectHeader) -> *const ObjectHeader {
    class_instance_prototype(obj)
}

#[inline]
fn word_object(word: u64) -> *const ObjectHeader {
    let v = crate::JSValue::from_bits(word);
    if v.is_pointer() {
        v.as_pointer::<ObjectHeader>() as *const ObjectHeader
    } else {
        std::ptr::null()
    }
}

/// [`class_instance_prototype`], building the class's prototype object first
/// when it does not exist yet (as any `C.prototype` read does). Allocates: the
/// caller roots what it holds.
///
/// # Safety
/// `obj` is a live object.
#[inline(never)]
pub(crate) unsafe fn class_instance_prototype_built(
    obj: *const ObjectHeader,
) -> *const ObjectHeader {
    let recorded = crate::object::shapes::object_prototype_word(obj);
    if recorded != 0 {
        return word_object(recorded);
    }
    let Some(class_id) = shape_named_class(obj) else {
        return std::ptr::null();
    };
    let word = crate::object::class_holder_word_selected(class_id);
    let existing = word_object(word);
    if !existing.is_null() {
        return existing;
    }
    word_object(
        crate::object::class_registry::class_decl_prototype_value_selected(class_id).to_bits(),
    )
}

/// The iterator-helper names (`crate::iterator_helpers::is_iterator_helper_method`),
/// whose call the tower routes by whether the receiver is an iterator, which
/// the receiver predicate does not pin. (The `using` hooks are internal
/// `__perry_` names, never prototype properties.)
#[inline]
fn is_iterator_helper_name(name: &[u8]) -> bool {
    matches!(
        name,
        b"map"
            | b"filter"
            | b"take"
            | b"drop"
            | b"flatMap"
            | b"toArray"
            | b"forEach"
            | b"reduce"
            | b"some"
            | b"every"
            | b"find"
    )
}

/// Names that are not string-keyed properties of a class prototype: private
/// names and the internal aliases of symbol-keyed members
/// ([`call_non_property_member`]).
#[inline]
pub(crate) fn name_is_not_a_prototype_method(name: &[u8]) -> bool {
    name.is_empty() || is_non_property_member_name(name)
}

/// The fast form of a class instance's method call at the top of the tower:
/// an ordinary class instance (the receiver predicate the tower's per-receiver
/// probes reduce to, [`class_receiver_fast_guard`]) whose prototype exists, a
/// chain whose shapes name a DATA property for the name, holding a compiled
/// user function that takes the call's arguments as they are. Anything else
/// returns `None` and the tower runs.
///
/// # Safety
/// `args_ptr` holds `args_len` values.
#[inline]
pub(super) unsafe fn try_class_holder_fast_dispatch(
    object: f64,
    key: &MethodKey<'_>,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    let name = key.bytes;
    let recv = class_receiver_guard(object, key)?;
    let obj_addr = recv.addr;
    // `constructor` is the class itself, which the tower's call path
    // reports as not callable without `new`.
    if name_is_not_a_prototype_method(name) || name == b"constructor" {
        return None;
    }
    if is_iterator_helper_name(name) {
        return None;
    }
    let start = if recv.recorded_prototype.is_null() {
        guarded_class_instance_prototype(obj_addr as *const ObjectHeader)
    } else {
        recv.recorded_prototype
    };
    if start.is_null() {
        return None;
    }
    let ChainMethod::Data { value, body, .. } = chain_method(start, key) else {
        return None;
    };
    let (closure, info) = compiled_user_method(value, body)?;
    let _depth_guard = CallMethodDepthGuard::enter("")?;
    Some(crate::closure::call_compiled_body_this(
        closure,
        info,
        crate::closure::JsThis::from_f64(object),
        args_ptr,
        args_len,
    ))
}

/// `value`, and its body, when it is a compiled function the fast call may
/// enter with the receiver as `this` and the arguments as given: a function
/// object with a compiled body (`FN_COMPILED_BODY`: user code, never a
/// borrowed built-in), no rest or `arguments` bundling (which allocates while
/// the receiver is an unrooted local), and no captured `this` to rebind. When
/// the holder's shape names the slot's body (`body`), the slot is a function
/// object of it by that shape's fact, so the cell is not re-validated.
#[inline]
unsafe fn compiled_user_method(
    value: u64,
    body: Option<&'static crate::closure::JsFunctionInfo>,
) -> Option<(
    *const crate::closure::ClosureHeader,
    &'static crate::closure::JsFunctionInfo,
)> {
    if value & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return None;
    }
    let addr = (value & crate::value::POINTER_MASK) as usize;
    let info = match body {
        Some(info) => info,
        None => {
            if !crate::closure::is_closure_ptr(addr) {
                return None;
            }
            (*(addr as *const crate::closure::ClosureHeader))
                .info
                .as_ref()?
        }
    };
    if info.flags & crate::codegen_abi::FN_COMPILED_BODY == 0
        || crate::closure::info_rest(info).is_some()
    {
        return None;
    }
    let closure = addr as *const crate::closure::ClosureHeader;
    let raw_count = (*closure).capture_count;
    if raw_count & crate::closure::CAPTURES_THIS_FLAG != 0
        && raw_count & crate::closure::NO_THIS_REBIND_FLAG == 0
        && !crate::closure::closure_is_arrow(closure)
    {
        return None;
    }
    Some((closure, info))
}

/// Is `value` a user function rather than a realm built-in (whose native arm
/// the tower keeps)? A compiled body is user code by construction; anything
/// else asks the borrowed-builtin classifier.
#[inline]
fn is_user_function_value(value: u64, name: &[u8]) -> bool {
    if value & !crate::value::POINTER_MASK == crate::value::POINTER_TAG {
        let addr = (value & crate::value::POINTER_MASK) as usize;
        if crate::closure::is_closure_ptr(addr) {
            // SAFETY: a proven, live closure cell; a non-null info is static.
            if let Some(info) = unsafe {
                (*(addr as *const crate::closure::ClosureHeader))
                    .info
                    .as_ref()
            } {
                if info.flags & crate::codegen_abi::FN_COMPILED_BODY != 0 {
                    return true;
                }
            }
        }
    }
    std::str::from_utf8(name)
        .is_ok_and(|name| crate::array::value_is_own_user_method(f64::from_bits(value), name))
}

/// The receiver predicate of the fast call (see [`class_receiver_fast_guard`]),
/// with the own-key absence decided by the receiver's key list.
#[inline]
unsafe fn class_receiver_guard(object: f64, key: &MethodKey<'_>) -> Option<super::ClassReceiver> {
    super::class_receiver_fast_guard(object, key.bytes)
}

/// Call `value` (a property value the chain answered) as `recv`'s method,
/// exactly as the tower calls an inherited value: a closure that keeps `this`
/// in a capture is re-bound to the receiver first.
///
/// # Safety
/// `args_ptr` holds `args_len` values; `recv` is rooted by the caller.
#[inline(never)]
pub(crate) unsafe fn call_chain_value(
    value: u64,
    recv: &crate::gc::RuntimeHandle,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let bound = crate::closure::clone_closure_rebind_this(value, recv.get_nanbox_f64());
    crate::closure::native_call_value_this(
        f64::from_bits(bound),
        crate::closure::JsThis::from_f64(recv.get_nanbox_f64()),
        args_ptr,
        args_len,
    )
}

/// The value a class instance's chain gives `name`, by shapes where they
/// answer and by an ordinary [[Get]] (with the instance as the receiver an
/// accessor sees) where they do not. `None` when the chain has no such
/// property, a data property that is a built-in rather than a user function,
/// or no chain could be found. Builds the class's prototype object
/// when it does not exist yet; allocates and may run a getter.
///
/// # Safety
/// `recv` roots a live object receiver.
#[inline(never)]
pub(crate) unsafe fn class_instance_property_value(
    recv: &crate::gc::RuntimeHandle,
    key: &MethodKey<'_>,
) -> Option<u64> {
    let obj = crate::value::js_nanbox_get_pointer(recv.get_nanbox_f64()) as *const ObjectHeader;
    let start = class_instance_prototype_built(obj);
    if start.is_null() {
        return (crate::object::shapes::object_prototype_word(obj) == crate::value::TAG_NULL)
            .then_some(crate::value::TAG_UNDEFINED);
    }
    let holder = match chain_method(start, key) {
        // A built-in a prototype inherits from the realm (`toString` on
        // `%Object.prototype%`, a native base's methods) keeps the tower's
        // own native arms, exactly as before the chain answered.
        ChainMethod::Data { value, .. } => return Some(value),
        ChainMethod::Absent => return Some(crate::value::TAG_UNDEFINED),
        ChainMethod::Get { holder } => holder,
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let holder_h = scope.root_raw_mut_ptr(holder as *mut ObjectHeader);
    let method_key = scope.root_string_ptr(crate::string::js_string_from_bytes(
        key.bytes.as_ptr(),
        key.bytes.len() as u32,
    ));
    let receiver_f64 = recv.get_nanbox_f64();
    let override_scope = crate::gc::RuntimeHandleScope::new();
    let prev_override = super::super::field_get_set::accessor_receiver_override_begin(receiver_f64)
        .map(|value| override_scope.root_nanbox_f64(value));
    let value = holder_h.with_mut_ptr(|holder: *mut ObjectHeader| {
        method_key.with_const_ptr::<crate::StringHeader, _>(|key| {
            js_object_get_field_by_name(holder as *const _, key)
        })
    });
    super::super::field_get_set::accessor_receiver_override_end(
        prev_override.map(crate::gc::RuntimeHandle::into_nanbox_f64),
    );
    Some(value.bits())
}

/// The property's value when the tower's user-function arm may answer it.
pub(crate) unsafe fn class_instance_method_value(
    recv: &crate::gc::RuntimeHandle,
    key: &MethodKey<'_>,
) -> Option<u64> {
    let value = class_instance_property_value(recv, key)?;
    let name = std::str::from_utf8(key.bytes).ok()?;
    if super::is_self_redispatching_proto_method(f64::from_bits(value), name) {
        return None;
    }
    is_user_function_value(value, key.bytes).then_some(value)
}

/// A class member that is not a string-keyed property of its prototype: a
/// private method (`#m`, reached through its brand) or a symbol-keyed method
/// under its internal dispatch alias (`@@iterator`, `__perry_dispose__`, ...).
/// These live with the class's private and symbol members, not on the
/// prototype's key list, so the shape walk cannot name them; their own lanes
/// (brands, symbol members) retire them. Ordinary names return `None`.
///
/// # Safety
/// `args_ptr` holds `args_len` values.
#[inline(never)]
pub(crate) unsafe fn call_non_property_member(
    receiver: f64,
    class_id: u32,
    name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if class_id == 0 || !is_non_property_member_name(name.as_bytes()) {
        return None;
    }
    let (func_ptr, param_count, has_synthetic_arguments, has_rest) =
        crate::object::class_registry::class_method_slot_target(class_id, name)?;
    Some(crate::object::class_registry::call_vtable_method_value(
        func_ptr,
        receiver,
        args_ptr,
        args_len,
        param_count,
        has_synthetic_arguments,
        has_rest,
        None,
    ))
}

/// A private name or a symbol member's internal dispatch alias.
#[inline]
pub(crate) fn is_non_property_member_name(name: &[u8]) -> bool {
    !name.is_empty()
        && (name[0] == b'#' || name.starts_with(b"__perry_") || name.starts_with(b"@@"))
}

/// The dispatch tower's arm for a class instance (`class_id` non-zero): the
/// property its prototype chain's shapes name, called with the instance as
/// `this` (the class's prototype object built first if no read has built it
/// yet); else a member that is not a prototype property. `None` when neither
/// answers. Out of line so the tower's own frame stays small.
///
/// # Safety
/// `recv` roots a live object receiver; `args_ptr` holds `args_len` values,
/// which `arg_handles` roots.
#[inline(never)]
pub(crate) unsafe fn call_class_instance_member(
    recv: &crate::gc::RuntimeHandle,
    arg_handles: &[crate::gc::RuntimeHandle],
    class_id: u32,
    method_name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if class_id != 0 && !name_is_not_a_prototype_method(method_name.as_bytes()) {
        let key = MethodKey::bytes(method_name.as_bytes());
        let value = class_instance_method_value(recv, &key)?;
        let args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(arg_handles);
        return Some(call_chain_value(value, recv, args.as_ptr(), args.len()));
    }
    call_non_property_member(
        recv.get_nanbox_f64(),
        class_id,
        method_name,
        args_ptr,
        args_len,
    )
}

/// The tower's arm for a registered class id used as a receiver: the method
/// `C.prototype` holds for the name, called with an undefined `this`.
/// `None` when the prototype has no such user method.
///
/// # Safety
/// `arg_handles` roots the call's arguments.
#[inline(never)]
pub(crate) unsafe fn call_class_ref_method(
    class_id: u32,
    method_name: &str,
    arg_handles: &[crate::gc::RuntimeHandle],
) -> Option<f64> {
    if name_is_not_a_prototype_method(method_name.as_bytes()) {
        return None;
    }
    let proto = crate::object::class_registry::class_decl_prototype_value(class_id);
    let proto = crate::JSValue::from_bits(proto.to_bits());
    if !proto.is_pointer() {
        return None;
    }
    let key = MethodKey::bytes(method_name.as_bytes());
    let ChainMethod::Data { value, .. } = chain_method(proto.as_pointer::<ObjectHeader>(), &key)
    else {
        return None;
    };
    if !crate::array::value_is_own_user_method(f64::from_bits(value), method_name) {
        return None;
    }
    let args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(arg_handles);
    Some(crate::closure::native_call_value_this(
        f64::from_bits(value),
        crate::closure::JsThis::from_f64(f64::from_bits(crate::value::TAG_UNDEFINED)),
        args.as_ptr(),
        args.len(),
    ))
}

#[cfg(test)]
mod selection_tests;
