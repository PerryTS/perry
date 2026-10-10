//! Computed reads own the same shape/holder proofs as named reads. Four ways
//! belong to the site; the existing holder root list traces their keys and hops.
use super::*;

const WAYS: usize = 4;
const MAX_EVICTIONS: u32 = 16;

#[derive(Clone, Copy)]
pub(super) enum KeyRef<'a> {
    Name { word: u64, bytes: &'a [u8] },
    Symbol(u64),
}
impl KeyRef<'_> {
    pub(super) unsafe fn position(
        self,
        keys: *const crate::array::ArrayHeader,
        count: u32,
    ) -> Option<u32> {
        let (word, bytes) = match self {
            Self::Name { word, bytes } => (word, Some(bytes)),
            Self::Symbol(word) => (word, None),
        };
        let slots = crate::array::array_elements_ptr(keys);
        if word != 0 {
            if let Some(i) = (0..count).find(|&i| *slots.add(i as usize) == word) {
                return Some(i);
            }
        }
        bytes.and_then(|bytes| crate::object::keys_find_slot_by_bytes_resolved(keys, count, bytes))
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct KeyedEntry {
    holder: HolderEntry,
    key: u64,
}
#[repr(C)]
pub struct KeyedCache {
    entries: [KeyedEntry; WAYS],
    next: u32,
    evictions: u32,
    registered: bool,
}

// The emitted own-symbol guard reads the first way through these offsets.
const _: () = assert!(std::mem::offset_of!(KeyedCache, entries) == 0);
const _: () = assert!(std::mem::offset_of!(KeyedEntry, holder) == 0);
const _: () = assert!(std::mem::offset_of!(KeyedEntry, key) == 8 * 8);
const _: () = assert!(std::mem::size_of::<KeyedEntry>() == 9 * 8);

/// Only shape-matchable receiver words enter the hit path. Every other cell's
/// honest +4 word misses; no receiver classification is repeated on a hit.
#[inline(always)]
unsafe fn receiver(bits: u64) -> Option<*const ObjectHeader> {
    if bits >> 48 != 0x7FFD {
        return None;
    }
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    if addr < perry_abi::RECEIVER_HANDLE_FLOOR {
        return None;
    }
    Some(addr as *const ObjectHeader)
}

#[inline]
unsafe fn answer(cache: &KeyedCache, recv: *const ObjectHeader, key: u64) -> Option<u64> {
    let stamp = (*recv).parent_class_id;
    let token = (u64::from(stamp) | PIC_ID_TOKEN_BIT) as i64;
    for entry in &cache.entries {
        let c = &entry.holder;
        if entry.key != key || c[HOLDER_RECV] != token {
            continue;
        }
        // An own entry names no particular receiver. Its token pins the slot
        // and two receivers of the same shape load their respective values.
        if c[HOLDER_OBJ] == 0 {
            if c[HOLDER_KIND] == HOLDER_ABSENT_DEPTH1 {
                return Some(crate::value::TAG_UNDEFINED);
            }
            return holder_slot_value(recv as usize, c[HOLDER_KIND] as u32)
                .filter(|&v| v != crate::value::TAG_HOLE);
        }
        return saved_entry_answer(c, token, recv);
    }
    None
}

pub(super) unsafe fn walk_to_key(
    recv: *const ObjectHeader,
    key: KeyRef<'_>,
    class_first: bool,
    max_depth: usize,
) -> Option<Walk> {
    debug_assert!(max_depth <= WALK_HOPS + 1);
    let mut w = Walk {
        holder: 0,
        holder_shape: 0,
        slot: None,
        hops: NO_HOPS,
        depth: 0,
        getter: 0,
    };
    let object_prototype = crate::array::object_prototype_addr_if_resolved();
    let mut current = recv;
    for depth in 1..=max_depth {
        // `%Object.prototype%` is an immutable-prototype exotic object: its
        // [[Prototype]] is null for its whole life, whatever its shape's
        // identity word says, so reaching it ends the chain.
        let terminal = depth > 1 && current as usize == object_prototype;
        // `current`'s recorded word, once read for its identity check.
        let mut word = None;
        let pid = if terminal {
            PROTO_ID_NULL
        } else if depth == 1 && class_first {
            // A bare CLASS ShapeId does not pin the registry's live
            // C.prototype. The collecting class-read hit compares that
            // pointer on every use; this walk records it as the first hop.
            crate::object::shapes::PROTO_ID_CLASS
        } else {
            let (pid, w) = admitted_link(current)?;
            word = Some(w);
            pid
        };
        if pid == PROTO_ID_NULL {
            // `current` is the terminal object, and it lacks `name`.
            if depth == 1 {
                return None;
            }
            let (h, sh) = w.hops[depth - 2];
            w.hops[depth - 2] = (0, 0);
            w.holder = h;
            w.holder_shape = sh;
            w.depth = depth - 1;
            return Some(w);
        }
        let next = if depth == 1 && class_first {
            class_link(recv)?
        } else if pid == PROTO_ID_DEFAULT {
            object_prototype as *const ObjectHeader
        } else {
            match word {
                Some(w) => next_from_word(current, w),
                None => next_prototype(current),
            }
        };
        if next.is_null() || next == current || next == recv || !hop_admitted(next as usize) {
            return None;
        }
        let shape = object_shape_descriptor(next)?;
        if !shape.object_kind.is_ordinary_layout() || object_shape_stamp(next) == 0 {
            return None;
        }
        let keys = shape.keys as usize as *const crate::array::ArrayHeader;
        if !keys.is_null() {
            if let Some(s) = key.position(keys, shape.logical_key_count) {
                // This exact slot, not another name walk, supplies the data
                // admission proof kept by the existing holder memo. Its
                // ShapeId compare invalidates it when the entry changes.
                if crate::object::key_attrs::key_is_accessor_at(keys, s) {
                    return None;
                }
                let s = holder_slot_word(next as usize, s, shape.live_inline_slot_count)?;
                w.holder = next as usize;
                w.holder_shape = object_shape_stamp(next);
                w.slot = Some(s);
                w.depth = depth;
                return Some(w);
            }
        }
        if depth == max_depth {
            // One more object would be needed: either the holder or the null
            // link past the last hop.
            if next as usize != object_prototype && admitted_proto_id(next) != Some(PROTO_ID_NULL) {
                return None;
            }
            w.holder = next as usize;
            w.holder_shape = object_shape_stamp(next);
            w.depth = depth;
            return Some(w);
        }
        w.hops[depth - 1] = (next as usize, object_shape_stamp(next));
        current = next;
    }
    None
}

/// Shape evidence is re-read after Get, so no unrooted walk crosses user code.
unsafe fn dynamic_key_walk(recv: *const ObjectHeader, key: KeyRef<'_>) -> Option<Walk> {
    let recv = ordinary_receiver(recv as usize)?;
    match key {
        KeyRef::Name { bytes, .. } if !read_name_admitted(recv, bytes) => return None,
        KeyRef::Symbol(word)
            if crate::object::class_has_symbol_member_in_chain(
                (*recv).class_id,
                (word & crate::value::POINTER_MASK) as usize,
                false,
            ) =>
        {
            return None
        }
        _ => {}
    }
    if crate::process::is_process_env_ptr(recv as usize) {
        return None;
    }
    let shape = object_shape_descriptor(recv)?;
    // Native aliases can forward nullish own slots into native state. A keyed
    // proof must retain generic Get until it carries that forwarding contract.
    if shape.object_kind == crate::object::shapes::ShapeObjectKind::OrdinaryNativeAlias {
        return None;
    }
    let keys = shape.keys as usize as *const crate::array::ArrayHeader;
    if !keys.is_null() {
        if let Some(slot) = key.position(keys, shape.logical_key_count) {
            if crate::object::key_attrs::key_is_accessor_at(keys, slot) {
                return None;
            }
            let slot = holder_slot_word(recv as usize, slot, shape.live_inline_slot_count)?;
            return Some(Walk {
                holder: 0,
                holder_shape: 0,
                slot: Some(slot),
                hops: NO_HOPS,
                depth: 1,
                getter: 0,
            });
        }
    }
    if admitted_proto_id(recv)? == PROTO_ID_NULL {
        return Some(Walk {
            holder: 0,
            holder_shape: 0,
            slot: None,
            hops: NO_HOPS,
            depth: 1,
            getter: 0,
        });
    }
    walk_to_key(recv, key, false, HOLDER_MAX_DEPTH)
}

unsafe fn publish_keyed(cache: &mut KeyedCache, recv: *const ObjectHeader, key: u64, w: &Walk) {
    let token = (u64::from(object_shape_stamp(recv)) | PIC_ID_TOKEN_BIT) as i64;
    let available = cache.entries.iter().position(|e| {
        e.holder[HOLDER_RECV] == 0
            || (e.key == key && e.holder[HOLDER_RECV] == token)
            || crate::object::shapes::shape_is_retired(e.holder[HOLDER_RECV] as u32)
            || (e.holder[HOLDER_OBJ] != 0 && holder_entry_retired(&e.holder))
    });
    let i = if let Some(i) = available {
        i
    } else {
        if cache.evictions >= MAX_EVICTIONS {
            return;
        }
        cache.evictions += 1;
        let i = cache.next as usize;
        cache.next = (cache.next + 1) % WAYS as u32;
        i
    };
    let mut c = HolderEntry([0; HOLDER_STATE - HOLDER_RECV]);
    c[HOLDER_OBJ] = w.holder as i64;
    c[HOLDER_SHAPE] = (u64::from(w.holder_shape) | u64::from(w.hops[2].1) << 32) as i64;
    c[HOLDER_KIND] = match (w.depth, w.slot) {
        (1, Some(s)) => i64::from(s),
        (1, None) => HOLDER_ABSENT_DEPTH1,
        (d, s) => {
            (HOLDER_STUB
                | if s.is_none() { HOLDER_ABSENT_BIT } else { 0 }
                | (d as u64) << HOLDER_DEPTH_SHIFT
                | u64::from(s.unwrap_or(0))) as i64
        }
    };
    for h in 0..HOLDER_MAX_DEPTH - 1 {
        c[HOLDER_HOPS + h] = w.hops[h].0 as i64;
    }
    c[HOLDER_HOP_SHAPES] = (u64::from(w.hops[0].1) | u64::from(w.hops[1].1) << 32) as i64;
    c[HOLDER_RECV] = token;
    cache.entries[i] = KeyedEntry { holder: c, key };
    if !cache.registered {
        HOLDER_SITES
            .lock()
            .unwrap()
            .push(HolderSite::Keyed(cache as *mut _ as usize));
        cache.registered = true;
    }
}

pub(super) unsafe fn scan_roots(site: usize, visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let cache = &mut *(site as *mut KeyedCache);
    for e in &mut cache.entries {
        if e.holder[HOLDER_RECV] != 0 {
            visitor.visit_nanbox_u64_slot(&mut e.key);
            scan_entry_roots(&mut e.holder, visitor);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_object_get_field_by_key_site(
    slot: *mut *mut KeyedCache,
    site_id: u64,
    obj: *const ObjectHeader,
    key: f64,
    obj_box: f64,
) -> f64 {
    // Workers leave the primary-owned words entirely alone, including peek.
    if WORKER_AGENTS_EXIST.load(Ordering::SeqCst) == 0 {
        if let Some(recv) = receiver(obj_box.to_bits()) {
            let cache = crate::object::pic_slot_peek(slot);
            if !cache.is_null() {
                if let Some(bits) = answer(&*cache, recv, key.to_bits()) {
                    return f64::from_bits(bits);
                }
            }
        }
    }
    miss(slot, site_id, obj, key, obj_box, false)
}

/// Unknown-receiver reads retain the full dynamic dispatcher on a miss.
/// This is the same keyed record, not a second string or symbol IC.
#[no_mangle]
pub unsafe extern "C" fn js_dyn_index_get_site(
    slot: *mut *mut KeyedCache,
    obj_box: f64,
    key: f64,
) -> f64 {
    if WORKER_AGENTS_EXIST.load(Ordering::SeqCst) == 0 {
        if let Some(recv) = receiver(obj_box.to_bits()) {
            let cache = crate::object::pic_slot_peek(slot);
            if !cache.is_null() {
                if let Some(bits) = answer(&*cache, recv, key.to_bits()) {
                    return f64::from_bits(bits);
                }
            }
        }
    }
    let obj = (obj_box.to_bits() & crate::value::POINTER_MASK) as *const ObjectHeader;
    miss(slot, 0, obj, key, obj_box, true)
}

#[cold]
#[inline(never)]
unsafe fn miss(
    slot: *mut *mut KeyedCache,
    site_id: u64,
    obj: *const ObjectHeader,
    key: f64,
    obj_box: f64,
    dynamic: bool,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let recv_handle = scope.root_nanbox_f64(obj_box);
    let key_handle = scope.root_nanbox_f64(key);
    let symbol = crate::symbol::js_is_symbol(key) != 0;
    let value = if dynamic || receiver(obj_box.to_bits()).is_none() {
        crate::value::js_dyn_index_get(obj_box, key)
    } else if symbol {
        crate::symbol::js_object_get_symbol_property(obj_box, key)
    } else {
        crate::object::dynamic_key_read::js_typed_feedback_object_get_field_by_key_f64(
            site_id, obj, key, obj_box,
        )
    };
    if WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0
        || crate::agent::current_agent() != crate::agent::PRIMARY_AGENT
        || slot.is_null()
        || crate::object::field_get_set::accessor_receiver_override_armed()
        || crate::object::prototype_chain::resolution_stack_savepoint() != 0
    {
        return value;
    }
    // A saturated site keeps its remaining hits, but its misses use only Get.
    // Do not rebuild a holder proof that the bounded memo cannot publish.
    let cache = crate::object::pic_slot_peek(slot);
    if !cache.is_null() && (*cache).evictions >= MAX_EVICTIONS {
        return value;
    }
    let bits = key_handle.get_nanbox_u64();
    let tag = bits >> 48;
    if !symbol && tag != 0x7FFF && tag != 0x7FF9 {
        return value;
    }
    let Some(recv) = receiver(recv_handle.get_nanbox_u64()) else {
        return value;
    };
    let mut buf = [0; crate::value::SHORT_STRING_MAX_LEN];
    let bytes = if symbol {
        None
    } else {
        crate::string::js_string_key_bytes(crate::JSValue::from_bits(bits), &mut buf)
    };
    let key_ref = if symbol {
        KeyRef::Symbol(bits)
    } else {
        let Some(bytes) = bytes else {
            return value;
        };
        KeyRef::Name { word: bits, bytes }
    };
    let Some(w) = dynamic_key_walk(recv, key_ref) else {
        return value;
    };
    let confirmed = match w.slot {
        Some(s) => {
            holder_slot_value(
                if w.holder == 0 {
                    recv as usize
                } else {
                    w.holder
                },
                s,
            ) == Some(value.to_bits())
                && value.to_bits() != crate::value::TAG_HOLE
        }
        None => value.to_bits() == crate::value::TAG_UNDEFINED,
    };
    if confirmed {
        let canonical = if let Some(bytes) = bytes {
            let hash = crate::object::key_bytes_hash(bytes.as_ptr(), bytes.len());
            if tag == 0x7FF9 {
                bits
            } else {
                crate::string::atom_lookup(bytes, hash)
                    .map_or(bits, |a| crate::value::STRING_TAG | a as u64)
            }
        } else {
            bits
        };
        let cache = crate::object::pic_slot_resolve(slot);
        publish_keyed(&mut *cache, recv, canonical, &w);
    }
    value
}

#[cfg(test)]
pub(crate) unsafe fn test_answer(
    cache: *const KeyedCache,
    recv: *const ObjectHeader,
    key: u64,
) -> Option<u64> {
    answer(&*cache, recv, key)
}

#[cfg(test)]
mod tests;
