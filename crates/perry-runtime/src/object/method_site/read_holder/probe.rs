//! Non-observable inspection of the same read-site entries. A builtin may
//! omit a Get only after its data value or getter is known; probing never
//! invokes a getter. Entries and roots belong to the ordinary read PIC.
use super::*;

#[derive(Clone, Copy)]
pub(crate) enum Key<'a> {
    Name(&'a [u8]),
    Symbol(usize),
}

#[derive(Clone, Copy)]
pub(crate) enum Answer {
    Data(u64),
    Getter(u64),
}

#[inline]
unsafe fn getter(pair: usize) -> Answer {
    // The primed accessor lane owns an immutable pair. This is the same
    // direct slot load used by holder_closure_getter_call, not an array Get.
    let slots = crate::array::array_elements_ptr(pair as *const crate::array::ArrayHeader);
    Answer::Getter(*slots.add(crate::object::accessor_pair::PAIR_GET))
}

unsafe fn valid_pair(lane: u64) -> Option<usize> {
    if lane & crate::value::TAG_MASK != crate::value::POINTER_TAG {
        return None;
    }
    let pair = (lane & crate::value::POINTER_MASK) as usize;
    let header = crate::value::addr_class::try_read_gc_header(pair)?;
    if header.obj_type != crate::gc::GC_TYPE_ARRAY
        || (*(pair as *const crate::array::ArrayHeader)).length < 2
    {
        return None;
    }
    Some(pair)
}

/// Accessor admission shares the collecting read's receiver, hop, holder
/// and lane checks. The returned getter is borrowed until the next safepoint.
pub(crate) unsafe fn probe_accessor_entry(
    c: &PicCache,
    recv: *const ObjectHeader,
) -> Option<Answer> {
    let pair = accessor_entry_pair(recv, c)?;
    Some(getter(pair))
}

/// The native entry of an immutable getter closure, or zero for a value
/// that cannot establish a native getter. This is a scalar, never a GC edge.
pub(crate) fn getter_code(bits: u64) -> usize {
    if bits & crate::value::TAG_MASK != crate::value::POINTER_TAG {
        return 0;
    }
    crate::closure::get_valid_func_ptr(
        (bits & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader,
    ) as usize
}

/// Reuse precisely the collecting accessor read's lane validation before
/// reading its memoized immutable closure code.
pub(crate) unsafe fn probe_accessor_code(c: &PicCache, recv: *const ObjectHeader) -> Option<usize> {
    accessor_entry_pair(recv, c)?;
    Some(c[HOLDER_HOPS + 1] as usize)
}

unsafe fn position(obj: *const ObjectHeader, key: Key<'_>) -> Option<u32> {
    let keys = crate::object::object_keys(obj);
    match key {
        Key::Name(name) => {
            crate::object::keys_find_property_slot_by_bytes(keys.arr(), keys.count(), name)
        }
        Key::Symbol(symbol) => crate::object::shaped_symbols::position(obj, symbol),
    }
}

/// Prime only from shape facts, without running the Get. Refusals leave the
/// operation's normal observable Get untouched. Allocation here is native
/// PIC storage only; no managed object is allocated or moved.
pub(crate) unsafe fn prime(
    obj: *const ObjectHeader,
    key: Key<'_>,
    cache_slot: *mut PicCacheSlot,
) -> Option<Answer> {
    if WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0
        || crate::agent::current_agent() != crate::agent::PRIMARY_AGENT
        || crate::object::field_get_set::accessor_receiver_override_armed()
        || crate::object::prototype_chain::resolution_stack_savepoint() != 0
    {
        return None;
    }
    let recv = ordinary_receiver(obj as usize)?;
    if let Key::Name(name) = key {
        if !read_name_admitted(recv, name) {
            return None;
        }
    }
    let mut current = recv;
    let mut hops = [(0, 0); HOLDER_MAX_DEPTH - 1];
    for depth in 0..=HOLDER_MAX_DEPTH {
        let current_obj = ordinary_receiver(current as usize)?;
        let shape = object_shape_descriptor(current_obj)?;
        if let Some(index) = position(current_obj, key) {
            let attrs = crate::object::key_attrs::keys_entry(
                shape.keys as usize as *const crate::array::ArrayHeader,
                index,
            );
            if attrs & crate::object::key_attrs::ENTRY_PRIVATE != 0 {
                return None;
            }
            let slot = holder_slot_word(current_obj as usize, index, shape.live_inline_slot_count)?;
            let lane = holder_slot_value(current_obj as usize, slot)?;
            let is_accessor = attrs & crate::object::key_attrs::ENTRY_ACCESSOR != 0;
            // Own data uses RuntimeReadSite's compact entry. Own spill reads
            // may decline here; no receiver is kept alive just to cache it.
            if depth == 0 {
                return Some(if is_accessor {
                    getter(valid_pair(lane)?)
                } else {
                    Answer::Data(lane)
                });
            }
            let cache = crate::object::field_get_set::pic_slot_resolve::<PicCache>(cache_slot);
            if cache.is_null() {
                return None;
            }
            let mut w = Walk {
                holder: current_obj as usize,
                holder_shape: object_shape_stamp(current_obj),
                slot: Some(slot),
                hops: NO_HOPS,
                depth: depth.max(1),
                getter: 0,
            };
            let answer = if is_accessor {
                let pair = valid_pair(lane)?;
                w.hops[0].0 = pair;
                w.getter = match key {
                    Key::Name(_) => crate::object::accessor_pair::site_getter_word_of_value(lane)?,
                    Key::Symbol(_) => crate::object::accessor_pair::holder_closure_getter_entry(),
                };
                w.depth = 1;
                getter(pair)
            } else {
                for (to, from) in w.hops.iter_mut().zip(hops) {
                    *to = from;
                }
                Answer::Data(lane)
            };
            publish(cache, recv, &w, is_accessor);
            if is_accessor && depth > 1 {
                class_read::publish_accessor_hops(cache, &hops, depth - 1);
                (*cache)[HOLDER_KIND] |= HOLDER_ACCESSOR_DEEP as i64;
                (*cache)[HOLDER_HOP_SHAPES] = 0;
            }
            return Some(answer);
        }
        if depth == HOLDER_MAX_DEPTH {
            return None;
        }
        if depth > 0 {
            hops[depth - 1] = (current_obj as usize, object_shape_stamp(current_obj));
        }
        current = accessor_holder(current_obj)?.0;
    }
    None
}
