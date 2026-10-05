//! The emitter's shape-answered property access: its state keys through
//! their site memos (`object::own_slot_memo`) and `_events`' keys through
//! that object's own key list, with no `[[Get]]` / `[[Set]]` walk when the
//! shapes answer.

use super::*;

/// `value` as a live ordinary object: an arena cell whose header says an
/// unforwarded `GC_TYPE_OBJECT`, with its header's object flags.
pub(super) fn ordinary_object(value: f64) -> Option<(*mut crate::object::ObjectHeader, u16)> {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_pointer() {
        return None;
    }
    let addr = jsval.as_pointer::<u8>() as usize;
    if !crate::value::addr_class::is_plausible_heap_addr(addr)
        || crate::arena::classify_heap_generation(addr) == crate::arena::HeapGeneration::Unknown
    {
        return None;
    }
    // SAFETY: plausible and arena-owned, so the header is readable.
    let header = unsafe { crate::value::addr_class::try_read_gc_header_known_plausible(addr) }?;
    (header.obj_type == crate::gc::GC_TYPE_OBJECT
        && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0)
        .then_some((addr as *mut crate::object::ObjectHeader, header._reserved))
}

/// The bits inline slot `slot` of `obj` holds.
///
/// # Safety
/// `obj` is a live object with at least `slot + 1` inline slots.
#[inline]
pub(super) unsafe fn slot_bits(obj: *const crate::object::ObjectHeader, slot: u32) -> u64 {
    let fields = (obj as *const u8).add(std::mem::size_of::<crate::object::ObjectHeader>());
    std::ptr::read((fields as *const u64).add(slot as usize))
}

/// What `obj`'s shape says about its own key `key`: an inline data slot, or
/// absent from a `[[Prototype]]`-null object (a definite miss), or nothing
/// (the full `[[Get]]` / `[[Set]]` answers).
pub(super) enum OwnKey {
    Slot(*mut crate::object::ObjectHeader, u32),
    Absent,
    Unknown,
}

/// `target`'s own data property named by the string value `key_bits`
/// (`bytes`), answered by its shape's own key list alone: attributes that
/// live in that list (an identity fact of the shape), no indexed elements or
/// accessor record, an ordinary-band ShapeId. `Some((obj, flags, None))` when
/// the own list lacks the key. Reads no key other than the list's and
/// allocates nothing.
pub(super) fn own_slot_of(
    target: f64,
    key_bits: u64,
    bytes: &[u8],
) -> Option<(*mut crate::object::ObjectHeader, u16, Option<u32>)> {
    let (obj, flags) = ordinary_object(target)?;
    own_slot_in(obj, flags, key_bits, bytes).map(|found| (obj, flags, found))
}

/// [`own_slot_of`] for an object [`ordinary_object`] already validated
/// (`flags` its header's object flags), with nothing allocated since.
pub(super) fn own_slot_in(
    obj: *mut crate::object::ObjectHeader,
    flags: u16,
    key_bits: u64,
    bytes: &[u8],
) -> Option<Option<u32>> {
    // SAFETY: a live ordinary object the caller validated.
    if flags & crate::gc::OBJ_FLAG_HAS_DESCRIPTORS != 0
        && !unsafe { crate::object::key_attrs::attrs_live_in_keys(obj as usize) }
    {
        return None;
    }
    // SAFETY: as above; the shape record and its key list are read
    // immediately, with no allocation in between.
    unsafe {
        let meta = (*obj).meta;
        if !meta.is_null() && ((*meta).elements != 0 || (*meta).accessor_key_bits != 0) {
            return None;
        }
        let id = (*obj).parent_class_id;
        if !crate::object::shapes::is_site_matchable_shape_id(id) {
            return None;
        }
        let record = crate::object::shapes::shape_record_by_id(id)?;
        match record.own_data_slot_of_value(key_bits, bytes)? {
            Some(slot) if slot_bits(obj, slot) == crate::value::TAG_HOLE => None,
            found => Some(found),
        }
    }
}

/// `target`'s `[[Prototype]]` as its shape names it, when that is an object.
pub(super) fn shape_prototype(obj: *const crate::object::ObjectHeader) -> Option<f64> {
    // SAFETY: a live ordinary object the caller validated.
    let word = unsafe { crate::object::shapes::object_prototype_word(obj) };
    let value = JSValue::from_bits(word);
    (word != 0 && value.is_pointer()).then_some(f64::from_bits(word))
}

/// Is `obj`'s `[[Prototype]]` null?
pub(super) fn null_prototype(obj: *const crate::object::ObjectHeader, flags: u16) -> bool {
    // SAFETY: a live ordinary object the caller validated.
    let word = unsafe { crate::object::shapes::object_prototype_word(obj) };
    word == crate::value::TAG_NULL || (word == 0 && flags & crate::gc::OBJ_FLAG_NULL_PROTO != 0)
}

/// What `target`'s shape says about its own key `key` ([`own_slot_of`]); a
/// miss is definite only on a null `[[Prototype]]` (an emitter's `_events`).
pub(super) fn own_key(target: f64, key_bits: u64, bytes: &[u8]) -> OwnKey {
    match own_slot_of(target, key_bits, bytes) {
        Some((obj, _, Some(slot))) => OwnKey::Slot(obj, slot),
        Some((obj, flags, None)) if null_prototype(obj, flags) => OwnKey::Absent,
        _ => OwnKey::Unknown,
    }
}

thread_local! {
    /// The method resolver's site memo. Each way also validates the pooled
    /// method id; two names on identical receiver/holder words cannot alias.
    static EMIT_METHOD: crate::object::method_site::own_slot_memo::ProtoSlotMemo =
        const { crate::object::method_site::own_slot_memo::ProtoSlotMemo::new() };
}

/// `recv[name]` when the shapes answer it as an own data property of the
/// `[[Prototype]]` `recv`'s shape names while `recv`'s own list lacks it
/// (every emitter's), through the memo of the receiver words that proved it.
/// `None` otherwise.
pub(in crate::node_stream) fn shape_method(recv: f64, key: i64, name: &[u8]) -> Option<f64> {
    let (obj, _) = ordinary_object(recv)?;
    // SAFETY: a live ordinary object (validated above).
    if let Some((holder, slot)) = EMIT_METHOD.with(|memo| unsafe { memo.slot(obj, key) }) {
        // SAFETY: the memo's words pin `slot` as an inline slot of `holder`.
        let bits = unsafe { slot_bits(holder, slot) };
        return (bits != crate::value::TAG_HOLE).then_some(f64::from_bits(bits));
    }
    let (_, _, own) = own_slot_of(recv, 0, name)?;
    if own.is_some() {
        return None;
    }
    let proto = shape_prototype(obj)?;
    let (holder, _, slot) = own_slot_of(proto, 0, name)?;
    let slot = slot?;
    // SAFETY: `own_slot_of` proved the requested name absent from `obj`'s own list and
    // own plain data at inline slot `slot` of the prototype its shape names.
    EMIT_METHOD.with(|memo| unsafe { memo.prime(obj, holder, key, slot) });
    // SAFETY: `own_slot_of` resolved `slot` on the live holder.
    Some(f64::from_bits(unsafe { slot_bits(holder, slot) }))
}

/// `target[name]` for a name fixed in this file, when `target`'s shape
/// answers it ([`own_key`]); `None` otherwise.
pub(super) fn own_get(target: f64, key_bits: u64, bytes: &[u8]) -> Option<f64> {
    match own_key(target, key_bits, bytes) {
        // SAFETY: `own_key` resolved `slot` from the live object's shape.
        OwnKey::Slot(obj, slot) => Some(f64::from_bits(unsafe { slot_bits(obj, slot) })),
        OwnKey::Absent => Some(undefined_value()),
        OwnKey::Unknown => None,
    }
}

thread_local! {
    /// The emitter state keys' site memos (`object::own_slot_memo`): the
    /// receiver words on which each is an own plain data slot.
    pub(super) static EVENTS_SLOT: crate::object::method_site::own_slot_memo::OwnSlotMemo =
        const { crate::object::method_site::own_slot_memo::OwnSlotMemo::new() };
    pub(super) static EVENTS_COUNT_SLOT: crate::object::method_site::own_slot_memo::OwnSlotMemo =
        const { crate::object::method_site::own_slot_memo::OwnSlotMemo::new() };
    pub(super) static MAX_LISTENERS_SLOT: crate::object::method_site::own_slot_memo::OwnSlotMemo =
        const { crate::object::method_site::own_slot_memo::OwnSlotMemo::new() };
}

pub(super) type StateMemo =
    std::thread::LocalKey<crate::object::method_site::own_slot_memo::OwnSlotMemo>;

/// The inline slot of the state key `memo` remembers on `target`, priming
/// the memo from the shape's own key list on a miss. `None` when `target`'s
/// shape does not hold the key as own plain inline data.
pub(super) fn state_slot(
    target: f64,
    memo: &'static StateMemo,
    name: &[u8],
) -> Option<(*mut crate::object::ObjectHeader, u32, u16)> {
    let (obj, flags) = ordinary_object(target)?;
    state_slot_in(target, obj, memo, name).map(|slot| (obj, slot, flags))
}

/// [`state_slot`] for `target`, already validated as the ordinary object
/// `obj` with nothing allocated since.
pub(super) fn state_slot_in(
    target: f64,
    obj: *mut crate::object::ObjectHeader,
    memo: &'static StateMemo,
    name: &[u8],
) -> Option<u32> {
    // SAFETY: a live ordinary object the caller validated.
    let slot = match memo.with(|memo| unsafe { memo.slot(obj) }) {
        Some(slot) => slot,
        None => {
            let (own, slot, live) = own_data_slot(target, name)??;
            if own as usize != obj as usize || slot >= live {
                return None;
            }
            // SAFETY: `own_data_slot` proved `name` own plain data at inline
            // slot `slot` of the object's shape.
            memo.with(|memo| unsafe { memo.prime(obj, slot) });
            slot
        }
    };
    // SAFETY: the memo's word (or the proof above) pins `slot` as an inline
    // slot of this object's shape.
    (unsafe { slot_bits(obj, slot) } != crate::value::TAG_HOLE).then_some(slot)
}

/// One of the emitter state keys of `target` (`_events`, `_eventsCount`,
/// `_maxListeners`): its own slot when the shape holds it, else `[[Get]]`.
pub(super) fn state_get(target: f64, memo: &'static StateMemo, name: &[u8]) -> f64 {
    match state_slot(target, memo, name) {
        // SAFETY: `state_slot` resolved `slot` on the live object.
        Some((obj, slot, _)) => f64::from_bits(unsafe { slot_bits(obj, slot) }),
        None => get_named(target, name),
    }
}

/// `target.<state key> = value`: an overwrite of its own slot when the shape
/// holds it and the receiver is not frozen, else `[[Set]]`.
pub(super) fn state_set(target: f64, memo: &'static StateMemo, name: &[u8], value: f64) {
    if let Some((obj, slot, flags)) = state_slot(target, memo, name) {
        if flags & crate::gc::OBJ_FLAG_FROZEN == 0 {
            // SAFETY: an own plain data slot of the live object; the funnel
            // checks the field representation and runs the barrier.
            unsafe { crate::object::store_object_field_slot(obj, slot as usize, value.to_bits()) };
            return;
        }
    }
    set_named(target, name, value);
}

/// `target[key] = value` answered by `target`'s shape: an overwrite of an
/// own data slot (on a receiver with no descriptor and not frozen), or, on a
/// null-prototype object whose own list lacks the key (nothing on a chain can
/// intercept the add), the transition-cache key add. `false`, having stored
/// nothing, otherwise.
pub(super) fn shape_set(target: f64, key_bits: u64, bytes: &[u8], value: f64) -> bool {
    let Some((obj, flags, found)) = own_slot_of(target, key_bits, bytes) else {
        return false;
    };
    match found {
        Some(slot) => {
            if flags & (crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_HAS_DESCRIPTORS) != 0 {
                return false;
            }
            // SAFETY: `own_slot_of` resolved the own data slot on the live
            // object; the funnel checks the representation and barriers.
            unsafe { crate::object::store_object_field_slot(obj, slot as usize, value.to_bits()) };
            true
        }
        None => {
            if !null_prototype(obj, flags) {
                return false;
            }
            // The transition cache is keyed by the interned heap string.
            let key = if key_bits & !crate::value::POINTER_MASK == crate::value::STRING_TAG {
                (key_bits & crate::value::POINTER_MASK) as *const crate::StringHeader
            } else {
                match crate::string::intern_lookup_bytes(bytes) {
                    Some(key) => key,
                    None => return false,
                }
            };
            let mut refresh = None;
            crate::object::object_set_field_by_name_transition_only_fast_value(
                obj,
                key,
                value,
                &mut refresh,
            )
            .is_some()
        }
    }
}

// ─────────────────────────────────────────────────────────────────
// The common listener operations, answered by the two shapes involved (the
// emitter's and its `_events`'), each object validated once. Every one
// returns "not handled", having changed nothing, when any step needs node's
// full algorithm (a `newListener` / `removeListener` listener, a once
// wrapper, a listener array, the last listener, a frozen or descriptor-
// carrying object, a key its shape cannot answer); the caller then runs it.
// ─────────────────────────────────────────────────────────────────

const ADD_REFUSING_FLAGS: u16 = crate::gc::OBJ_FLAG_FROZEN
    | crate::gc::OBJ_FLAG_SEALED
    | crate::gc::OBJ_FLAG_NO_EXTEND
    | crate::gc::OBJ_FLAG_HAS_DESCRIPTORS;

/// An emitter and its `_events` as the fast operations see them.
struct EmitterView {
    target: (*mut crate::object::ObjectHeader, u16),
    count_slot: u32,
    events: f64,
    events_obj: (*mut crate::object::ObjectHeader, u16),
}

/// `target` as an ordinary emitter whose state keys are own plain data and
/// whose `_events` is an ordinary null-prototype object with no meta-event
/// listener named `meta` (`newListener` / `removeListener`).
fn emitter_view(target: f64, meta: &[u8]) -> Option<EmitterView> {
    let (obj, flags) = ordinary_object(target)?;
    if flags & ADD_REFUSING_FLAGS != 0 {
        return None;
    }
    let events_slot = state_slot_in(target, obj, &EVENTS_SLOT, EVENTS_KEY)?;
    let count_slot = state_slot_in(target, obj, &EVENTS_COUNT_SLOT, EVENTS_COUNT_KEY)?;
    // SAFETY: the memo resolved the slot on the live object.
    let events = f64::from_bits(unsafe { slot_bits(obj, events_slot) });
    let (events_obj, events_flags) = ordinary_object(events)?;
    if events_flags & ADD_REFUSING_FLAGS != 0
        || !null_prototype(events_obj, events_flags)
        || !meta_event_absent(events_obj, events_flags, meta)
    {
        return None;
    }
    Some(EmitterView {
        target: (obj, flags),
        count_slot,
        events,
        events_obj: (events_obj, events_flags),
    })
}

thread_local! {
    /// The `_events` words with no `newListener` / `removeListener` key.
    static NO_NEW_LISTENER: crate::object::method_site::own_slot_memo::AbsentKeyMemo =
        const { crate::object::method_site::own_slot_memo::AbsentKeyMemo::new() };
    static NO_REMOVE_LISTENER: crate::object::method_site::own_slot_memo::AbsentKeyMemo =
        const { crate::object::method_site::own_slot_memo::AbsentKeyMemo::new() };
}

/// Does `_events` (validated, `flags` its header flags) lack the meta-event
/// key `meta` as its own? Through the memo of the words that proved it.
fn meta_event_absent(events: *mut crate::object::ObjectHeader, flags: u16, meta: &[u8]) -> bool {
    let memo = if meta == b"newListener" {
        &NO_NEW_LISTENER
    } else {
        &NO_REMOVE_LISTENER
    };
    // SAFETY: a live ordinary object the caller validated.
    if memo.with(|memo| unsafe { memo.absent(events) }) {
        return true;
    }
    if own_slot_in(events, flags, 0, meta) != Some(None) {
        return false;
    }
    // SAFETY: as above; the shape's own list lacks the key.
    memo.with(|memo| unsafe { memo.prime(events) });
    true
}

/// `this._eventsCount = n` on a validated emitter.
fn store_count(view: &EmitterView, count: f64) {
    // SAFETY: the count slot is an own plain data slot of the live emitter.
    unsafe {
        crate::object::store_object_field_slot(
            view.target.0,
            view.count_slot as usize,
            count.to_bits(),
        )
    };
}

/// node's `_addListener(target, type, listener, prepend)` when `type` has no
/// listener yet: `events[type] = listener; ++this._eventsCount`.
pub(super) fn add_first_listener_fast(target: f64, event: f64, listener: f64) -> bool {
    let Some(view) = emitter_view(target, b"newListener") else {
        return false;
    };
    let (events_obj, events_flags) = view.events_obj;
    let key_bits = event.to_bits();
    // SAFETY: the `_events` object `emitter_view` validated.
    if unsafe { (*events_obj).class_id } != 0 {
        return false;
    }
    let key = with_string_bytes(event, |bytes| {
        if own_slot_in(events_obj, events_flags, key_bits, bytes) != Some(None) {
            return None;
        }
        // The key-add edges are keyed by the interned heap string.
        let heap = key_bits & !crate::value::POINTER_MASK == crate::value::STRING_TAG;
        let ptr = (key_bits & crate::value::POINTER_MASK) as *const crate::StringHeader;
        // SAFETY: a heap string value's pointer names a live string header.
        if heap && unsafe { string_is_interned(ptr) } {
            Some(ptr)
        } else {
            crate::string::intern_lookup_bytes(bytes)
        }
    });
    let Some(Some(key)) = key else {
        return false;
    };
    // SAFETY: the emitter's count slot, read before anything can allocate.
    let count = number_of(f64::from_bits(unsafe {
        slot_bits(view.target.0, view.count_slot)
    }));
    if count.is_nan() {
        return false;
    }
    // node stores the listener and then counts it; nothing can observe the
    // order here, and the count is stored first because the add may grow a
    // spill buffer (and so move the emitter). A refused add restores it.
    store_count(&view, count + 1.0);
    // SAFETY: `emitter_view` and the lookup above proved the add's
    // preconditions on `_events`: a class-less ordinary object, no frozen,
    // sealed, non-extensible or descriptor flag, a null `[[Prototype]]`, the
    // key absent from its own list and interned.
    if !unsafe { crate::object::add_absent_key_to_null_proto_object(events_obj, key, listener) } {
        store_count(&view, count);
        return false;
    }
    true
}

/// Is the heap string `key` the intern table's (a key-add edge's key)?
///
/// # Safety
/// `key` is a live heap string header.
unsafe fn string_is_interned(key: *const crate::StringHeader) -> bool {
    crate::value::addr_class::try_read_gc_header(key as usize).is_some_and(|header| {
        header.obj_type == crate::gc::GC_TYPE_STRING
            && header.gc_flags & crate::gc::GC_FLAG_INTERNED != 0
    })
}

/// node's `removeListener(type, listener)` when `type`'s one listener is
/// `listener` itself and another event keeps the emitter's map:
/// `--this._eventsCount; delete events[type]`.
pub(super) fn remove_only_listener_fast(target: f64, event: f64, listener: f64) -> bool {
    let Some(view) = emitter_view(target, b"removeListener") else {
        return false;
    };
    let (events_obj, events_flags) = view.events_obj;
    let key_bits = event.to_bits();
    let slot = with_string_bytes(event, |bytes| {
        own_slot_in(events_obj, events_flags, key_bits, bytes)
    });
    let Some(Some(Some(slot))) = slot else {
        return false;
    };
    // SAFETY: the slots were resolved on the live objects just validated.
    let (list, count) = unsafe {
        (
            slot_bits(events_obj, slot),
            number_of(f64::from_bits(slot_bits(view.target.0, view.count_slot))),
        )
    };
    if list != listener.to_bits() || !(count > 1.0) {
        return false;
    }
    store_count(&view, count - 1.0);
    // `delete events[type]`: the last-added key's rollback when it is one
    // (`object::delete_last_key`), else the full delete. Nothing reads the
    // emitter after it.
    // SAFETY: `_events` is the live object validated above, `slot` its key's
    // slot on its current shape; the delete re-validates the receiver.
    let deleted =
        unsafe { crate::object::delete_last_key::try_delete_last_added_key_at(events_obj, slot) };
    if deleted.is_none() {
        let _ = crate::object::js_object_delete_dynamic_value(view.events, event);
    }
    true
}

/// `this._events[type]` for an emitter whose shapes answer it: the stored
/// listener (a function or an array of them), `Some(undefined)` when none.
pub(super) fn listeners_of_fast(target: f64, event: f64) -> Option<f64> {
    let (obj, _) = ordinary_object(target)?;
    let events_slot = state_slot_in(target, obj, &EVENTS_SLOT, EVENTS_KEY)?;
    // SAFETY: the memo resolved the slot on the live object.
    let events = f64::from_bits(unsafe { slot_bits(obj, events_slot) });
    let (events_obj, events_flags) = ordinary_object(events)?;
    let key_bits = event.to_bits();
    match with_string_bytes(event, |bytes| {
        own_slot_in(events_obj, events_flags, key_bits, bytes)
    })?? {
        // SAFETY: resolved on the live `_events`.
        Some(slot) => Some(f64::from_bits(unsafe { slot_bits(events_obj, slot) })),
        None if null_prototype(events_obj, events_flags) => Some(undefined_value()),
        None => None,
    }
}

/// `this._events = events; this._eventsCount = 0` on an emitter whose state
/// keys are own plain data slots of a receiver that is not frozen (one
/// validation for both stores). `false`, having stored nothing, otherwise.
pub(super) fn reset_events_fast(target: f64, events: f64) -> bool {
    let Some((obj, flags)) = ordinary_object(target) else {
        return false;
    };
    if flags & crate::gc::OBJ_FLAG_FROZEN != 0 {
        return false;
    }
    let (Some(events_slot), Some(count_slot)) = (
        state_slot_in(target, obj, &EVENTS_SLOT, EVENTS_KEY),
        state_slot_in(target, obj, &EVENTS_COUNT_SLOT, EVENTS_COUNT_KEY),
    ) else {
        return false;
    };
    // SAFETY: own plain data slots of the live emitter; the funnel checks
    // the representation and runs the barrier.
    unsafe {
        crate::object::store_object_field_slot(obj, events_slot as usize, events.to_bits());
        crate::object::store_object_field_slot(obj, count_slot as usize, 0f64.to_bits());
    }
    true
}

#[cfg(test)]
mod method_body_tests {
    use super::*;

    #[test]
    fn method_fast_dispatch_follows_the_resolved_body_for_aliases_and_overrides() {
        let _global = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        let target = crate::node_stream::js_event_emitter_object_new(undefined_value());
        let builtin_proto = shape_prototype(ordinary_object(target).unwrap().0).unwrap();
        let emit = get_named(builtin_proto, b"emit");
        // Keep the aliases inline on a private prototype; the realm's
        // shared prototype may already be full after another unit test.
        let proto =
            crate::value::js_nanbox_pointer(crate::object::js_object_alloc_null_proto(0, 4) as i64);
        crate::object::js_object_set_prototype_of(target, proto);
        let alias_id = crate::value::JSValue::try_short_string(b"alias")
            .unwrap()
            .bits() as i64;
        let other_id = crate::value::JSValue::try_short_string(b"other")
            .unwrap()
            .bits() as i64;
        set_named(proto, b"alias", emit);
        set_named(proto, b"other", 42.0);
        let args = [f64::from_bits(
            crate::value::JSValue::try_short_string(b"none")
                .unwrap()
                .bits(),
        )];
        unsafe {
            assert_eq!(
                shape_method(target, alias_id, b"alias").unwrap().to_bits(),
                emit.to_bits()
            );
            assert_eq!(shape_method(target, other_id, b"other"), Some(42.0));
            let obj = ordinary_object(target).unwrap().0;
            set_named(proto, b"non_static_method_id", emit);
            let holder = ordinary_object(proto).unwrap().0;
            let heap_key = crate::string::intern_ascii_literal(b"non_static_method_id") as i64;
            let memo = crate::object::method_site::own_slot_memo::ProtoSlotMemo::new();
            memo.prime(obj, holder, heap_key, 2);
            assert!(
                memo.slot(obj, heap_key).is_none(),
                "heap ids cannot enter the memo"
            );
            // The same receiver/holder words cannot answer another method
            // from the memo's previous slot. The method id is part of each way.
            assert_eq!(
                shape_method(target, alias_id, b"alias").unwrap().to_bits(),
                emit.to_bits()
            );
            assert_eq!(
                crate::node_stream::emitter_emit_call(target, alias_id, b"alias", args.as_ptr(), 1)
                    .map(f64::to_bits),
                Some(crate::value::TAG_FALSE)
            );
            assert_eq!(
                crate::node_stream::emitter_emit_call(target, other_id, b"other", args.as_ptr(), 1),
                None
            );
            set_named(target, b"alias", 13.0);
            assert_eq!(
                crate::node_stream::emitter_emit_call(target, alias_id, b"alias", args.as_ptr(), 1),
                None
            );
        }
    }
}
