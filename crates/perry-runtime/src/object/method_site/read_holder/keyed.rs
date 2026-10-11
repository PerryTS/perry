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
        // Wide records already own a content-validated shape index. Do not
        // precede its lookup with an unbounded word scan of the same keys.
        if let Some(bytes) = bytes {
            if count >= crate::object::KEYS_INDEX_THRESHOLD {
                let hash = crate::object::key_bytes_hash(bytes.as_ptr(), bytes.len());
                return crate::object::keys_find_property_slot_by_bytes_resolved_hashed(
                    keys, count, bytes, hash,
                );
            }
        }
        let slots = crate::array::array_elements_ptr(keys);
        if word != 0 {
            if let Some(i) = (0..count).rev().find(|&i| {
                *slots.add(i as usize) == word
                    && !crate::object::key_attrs::entry_is_private(
                        crate::object::key_attrs::keys_entry(keys, i),
                    )
            }) {
                return Some(i);
            }
        }
        bytes.and_then(|bytes| {
            let hash = crate::object::key_bytes_hash(bytes.as_ptr(), bytes.len());
            crate::object::keys_find_property_slot_by_bytes_resolved_hashed(
                keys, count, bytes, hash,
            )
        })
    }
}

use super::shared::SharedEntry as KeyedEntry;
#[repr(C)]
pub struct KeyedCache {
    entries: [KeyedEntry; WAYS],
    next: u32,
    evictions: u32,
    registered: bool,
}

#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(std::mem::offset_of!(KeyedCache, entries) == 0);
    assert!(std::mem::size_of::<KeyedEntry>() == 56);
};

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
        if entry.identity_key != key || entry.token != token {
            continue;
        }
        return entry.read_value(recv);
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

/// A data/absence proof contains no observable Get and crosses no safepoint.
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
        e.token == 0
            || (e.identity_key == key && e.token == token)
            || crate::object::shapes::shape_is_retired(e.token as u32)
            || (e.holder != 0 && crate::object::shapes::shape_is_retired(e.holder_shape))
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
    let old = &cache.entries[i];
    if old.depth > 1 {
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
            old.hops,
            old.depth - 1,
        )));
    }
    let depth = if w.holder == 0 { 0 } else { w.depth };
    let hops = if depth > 1 {
        Box::into_raw(Box::<[Hop]>::from(&w.hops[..depth - 1])) as *mut Hop
    } else {
        std::ptr::null_mut()
    };
    cache.entries[i] = KeyedEntry {
        token,
        identity_key: key,
        holder: w.holder,
        holder_shape: w.holder_shape,
        slot: w.slot.unwrap_or(0),
        absent: w.slot.is_none(),
        depth,
        hops,
        ..KeyedEntry::EMPTY
    };
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
        if e.token != 0 {
            e.scan(visitor);
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
    if let Some(value) = shape_read(slot, obj_box.to_bits(), key.to_bits()) {
        return f64::from_bits(value);
    }
    // All proof work precedes Get. A declined proof needs no roots or post-Get
    // re-walk: each collecting dispatch is a tail call with no raw use after it.
    if dynamic || receiver(obj_box.to_bits()).is_none() {
        crate::value::js_dyn_index_get(obj_box, key)
    } else if crate::symbol::js_is_symbol(key) != 0 {
        crate::symbol::js_object_get_symbol_property(obj_box, key)
    } else {
        crate::object::dynamic_key_read::js_typed_feedback_object_get_field_by_key_f64(
            site_id, obj, key, obj_box,
        )
    }
}

/// Consume the same proof that fills a shared entry. Saturated sites share
/// the dispatcher's plain own-slot proof without repeating holder admission.
unsafe fn shape_read(slot: *mut *mut KeyedCache, obj_bits: u64, bits: u64) -> Option<u64> {
    if WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0
        || crate::agent::current_agent() != crate::agent::PRIMARY_AGENT
        || slot.is_null()
        || crate::object::field_get_set::accessor_receiver_override_armed()
        || crate::object::prototype_chain::resolution_stack_savepoint() != 0
    {
        return None;
    }
    let tag = bits >> 48;
    let symbol =
        tag != 0x7FFF && tag != 0x7FF9 && crate::symbol::js_is_symbol(f64::from_bits(bits)) != 0;
    if !symbol && tag != 0x7FFF && tag != 0x7FF9 {
        return None;
    }
    let recv = receiver(obj_bits)?;
    let cache = crate::object::pic_slot_peek(slot);
    if !cache.is_null() && (*cache).evictions >= MAX_EVICTIONS {
        return if symbol {
            None
        } else {
            crate::object::dynamic_key_read::positional_slot_answer(recv, bits)
                .map(f64::to_bits)
                .filter(|&value| value != crate::value::TAG_HOLE)
        };
    }
    let mut buf = [0; crate::value::SHORT_STRING_MAX_LEN];
    let bytes = if symbol {
        None
    } else {
        Some(crate::string::js_string_key_bytes(
            crate::JSValue::from_bits(bits),
            &mut buf,
        )?)
    };
    let key_ref = match bytes {
        Some(bytes) => KeyRef::Name { word: bits, bytes },
        None => KeyRef::Symbol(bits),
    };
    // Publication already uses the atom identity when one exists. A runtime
    // string (for example typeof's result) can have the same contents but a
    // different word: consume that existing entry before rebuilding its proof.
    let canonical = match bytes {
        Some(bytes) if tag != 0x7FF9 => {
            let hash = crate::object::key_bytes_hash(bytes.as_ptr(), bytes.len());
            crate::string::atom_lookup(bytes, hash)
                .map_or(bits, |a| crate::value::STRING_TAG | a as u64)
        }
        _ => bits,
    };
    if canonical != bits && !cache.is_null() {
        if let Some(value) = answer(&*cache, recv, canonical) {
            return Some(value);
        }
    }
    let w = dynamic_key_walk(recv, key_ref)?;
    let value = match w.slot {
        Some(s) => holder_slot_value(
            if w.holder == 0 {
                recv as usize
            } else {
                w.holder
            },
            s,
        )?,
        None => crate::value::TAG_UNDEFINED,
    };
    if value == crate::value::TAG_HOLE {
        return None;
    }
    // System allocation only: no GC or user code between the proof and its
    // publication, so receiver/key/hop pointers need no handle-scope round trip.
    let cache = crate::object::pic_slot_resolve(slot);
    publish_keyed(&mut *cache, recv, canonical, &w);
    Some(value)
}

/// A statement run consumes the keyed entries published by its original
/// source-ordered Gets. Each distinct key keeps its existing four-way site,
/// so sibling keys do not evict receiver alternatives. Loads only on entry.
#[no_mangle]
pub unsafe extern "C" fn js_region_holder_read(
    slot: *mut *mut KeyedCache,
    obj_box: f64,
    keys: *const u64,
    count: u32,
    out: *mut u64,
    numeric: u32,
) -> u32 {
    if WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0
        || count == 0
        || count as usize > WAYS
        || slot.is_null()
    {
        return 0;
    }
    let Some(recv) = receiver(obj_box.to_bits()) else {
        return 0;
    };
    let token = (u64::from((*recv).parent_class_id) | PIC_ID_TOKEN_BIT) as i64;
    for i in 0..count as usize {
        let cache = crate::object::pic_slot_peek(slot.add(i));
        if cache.is_null() {
            return 0;
        }
        let key = *keys.add(i);
        let Some(bits) = (*cache)
            .entries
            .iter()
            .find(|e| e.identity_key == key && e.token == token)
            .and_then(|e| e.read_value(recv))
        else {
            return 0;
        };
        if numeric != 0
            && bits != crate::value::TAG_UNDEFINED
            && bits & 0x7fff_ffff_ffff_ffff >= 0x7ff9_0000_0000_0000
        {
            return 0;
        }
        *out.add(i) = bits;
    }
    1
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
