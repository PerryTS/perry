//! Bounded read-site facts for declared-class instances.
//!
//! A bare CLASS ShapeId identifies the receiver's own keys but does not pin
//! `C.prototype`: the class registry can replace that pointer without a
//! receiver restamp. These entries therefore run only from the collecting
//! miss arm, where they re-read the live direct prototype on every hit.
//! The entries belong to one PicCache site (word 2 points to its bounded
//! process-lifetime record), never to a process-global `(shape, key)` table.

use super::*;

const SITE_WORD: usize = 2; // the existing PIC's unused scratch word
const SITE_TAG: u64 = 0xA2C1_0000_0000_0000;
const STATE_CLASS_SITE: i64 = 4;
const WAYS: usize = 16;

#[derive(Clone, Copy)]
struct Entry {
    token: i64,
    class_id: u32,
    depth: u8,
    absent: bool,
    slot: u32,
    holder: usize,
    holder_shape: u32,
    hops: [(usize, u32); HOLDER_MAX_DEPTH - 1],
}

const EMPTY: Entry = Entry {
    token: 0,
    class_id: 0,
    depth: 0,
    absent: false,
    slot: 0,
    holder: 0,
    holder_shape: 0,
    hops: [(0, 0); HOLDER_MAX_DEPTH - 1],
};

struct Site {
    entries: [Entry; WAYS],
    next: usize,
}

per_test_global! {
    static PRIMES: AtomicU64 = AtomicU64::new(0);
    static HITS: AtomicU64 = AtomicU64::new(0);
    static ROOT_REWRITES: AtomicU64 = AtomicU64::new(0);
}

pub(super) fn stats() -> (u64, u64, u64) {
    (
        PRIMES.load(Ordering::Relaxed),
        HITS.load(Ordering::Relaxed),
        ROOT_REWRITES.load(Ordering::Relaxed),
    )
}

#[inline]
unsafe fn site(c: &PicCache) -> Option<&Site> {
    let word = c[SITE_WORD] as u64;
    if c[HOLDER_STATE] & STATE_CLASS_SITE == 0 || word & !crate::value::POINTER_MASK != SITE_TAG {
        return None;
    }
    Some(&*((word & crate::value::POINTER_MASK) as usize as *const Site))
}

/// The current link from an intermediate hop, computed by the same admitted
/// shape/prototype rule the prime walk used. A changed or exotic link declines.
unsafe fn admitted_next(hop: *const ObjectHeader) -> Option<usize> {
    let pid = shape_proto_id(object_shape_stamp(hop))?;
    if object_proto_id(hop) != pid {
        return None;
    }
    if !(pid == PROTO_ID_DEFAULT
        || pid == PROTO_ID_NULL
        || (PROTO_ID_MIXED..PROTO_ID_UNIQUE).contains(&pid)
        || (pid < crate::object::shapes::PROTO_ID_CLASS && pid != PROTO_ID_DEFAULT))
    {
        return None;
    }
    let next = if pid == PROTO_ID_DEFAULT {
        crate::array::object_prototype_addr_if_resolved()
    } else if pid == PROTO_ID_NULL {
        0
    } else {
        next_prototype(hop) as usize
    };
    (next != 0).then_some(next)
}

unsafe fn answer(e: &Entry, recv: *const ObjectHeader) -> Option<u64> {
    if e.token == 0
        || e.class_id != (*recv).class_id
        || e.token != (u64::from(object_shape_stamp(recv)) | PIC_ID_TOKEN_BIT) as i64
    {
        return None;
    }
    let direct = if e.depth == 1 { e.holder } else { e.hops[0].0 };
    if class_link(recv)? as usize != direct {
        return None;
    }
    let mut previous = 0usize;
    for i in 0..(e.depth as usize).saturating_sub(1) {
        let (addr, shape) = e.hops[i];
        if addr == 0 || shape_word(addr) != shape {
            return None;
        }
        if i != 0 && admitted_next(previous as *const ObjectHeader)? != addr {
            return None;
        }
        previous = addr;
    }
    if previous != 0 && admitted_next(previous as *const ObjectHeader)? != e.holder {
        return None;
    }
    if e.holder == 0 || shape_word(e.holder) != e.holder_shape {
        return None;
    }
    if e.absent {
        Some(crate::value::TAG_UNDEFINED)
    } else {
        let bits = slot_bits(e.holder, e.slot);
        // The generic inherited getter treats nullish/hole holder values as
        // a miss and may continue to a farther prototype. The holder's
        // ShapeId does not change on a value overwrite, so recheck each hit.
        (bits != crate::value::TAG_UNDEFINED
            && bits != crate::value::TAG_NULL
            && bits != crate::value::TAG_HOLE)
            .then_some(bits)
    }
}

/// A class entry is served on the collecting miss path only. The GC-leaf
/// front has no receiver-root/registry contract for bare CLASS prototypes.
pub(super) unsafe fn try_hit(
    recv: *const ObjectHeader,
    cache_slot: *mut PicCacheSlot,
) -> Option<crate::value::JSValue> {
    if cache_slot.is_null()
        || WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0
        || crate::agent::current_agent() != crate::agent::PRIMARY_AGENT
    {
        return None;
    }
    let cache = crate::object::field_get_set::pic_slot_peek::<PicCache>(cache_slot);
    if cache.is_null() {
        return None;
    }
    let s = site(&*cache)?;
    for e in &s.entries {
        if let Some(bits) = answer(e, recv) {
            HITS.fetch_add(1, Ordering::Relaxed);
            super::super::stats_report_enabled();
            return Some(crate::value::JSValue::from_bits(bits));
        }
    }
    None
}

/// Prime only after the generic getter's result has been compared with the
/// same live chain and slot. Returns None if this receiver has no class link.
pub(super) unsafe fn prime(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
    cache_slot: *mut PicCacheSlot,
    name: &[u8],
) -> Option<crate::value::JSValue> {
    if class_link(obj).is_none()
        || !holder_name_admitted(name)
        || key_may_be_accessor(obj, name)
        || walk(obj, name, true).is_none()
    {
        return None;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let handle = scope.root_raw_mut_ptr(obj as *mut ObjectHeader);
    let key_handle = scope.root_string_ptr(key);
    let (value, obj) = handle.across_mut::<ObjectHeader, _>(|| {
        crate::object::field_get_set::get_field_by_name_after_site_miss(obj, key)
    });
    if WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0 {
        return Some(value);
    }
    key_handle.with_const_ptr::<crate::StringHeader, _>(|key| {
        let name = crate::string::header_str_checked(key)?.as_bytes();
        let Some(obj) = ordinary_receiver(obj as usize) else {
            return Some(value);
        };
        let Some(w) = walk(obj, name, true) else {
            return Some(value);
        };
        let bits = value.bits();
        let confirmed = match w.slot {
            None => bits == crate::value::TAG_UNDEFINED,
            Some(slot) => {
                bits == slot_bits(w.holder, slot)
                    && bits != crate::value::TAG_HOLE
                    && bits != crate::value::TAG_UNDEFINED
                    && bits != crate::value::TAG_NULL
            }
        };
        if confirmed {
            let cache = crate::object::field_get_set::pic_slot_resolve::<PicCache>(cache_slot);
            if !cache.is_null() {
                publish(cache, obj, &w);
            }
        }
        Some(value)
    })
}

unsafe fn publish(cache: *mut PicCache, recv: *const ObjectHeader, w: &Walk) {
    let c = &mut *cache;
    // Other PIC users may leave scratch data in word 2. Only a marker that
    // we published together with a tagged pointer grants dereference rights.
    let tagged = c[SITE_WORD] as u64;
    let has_site =
        c[HOLDER_STATE] & STATE_CLASS_SITE != 0 && tagged & !crate::value::POINTER_MASK == SITE_TAG;
    let s = if !has_site {
        let new = Box::into_raw(Box::new(Site {
            entries: [EMPTY; WAYS],
            next: 0,
        }));
        if c[HOLDER_STATE] & STATE_REGISTERED == 0 {
            let mut sites = HOLDER_SITES
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            sites.push(cache as usize);
            c[HOLDER_STATE] |= STATE_REGISTERED;
        }
        let addr = new as usize as u64;
        assert_eq!(addr & !crate::value::POINTER_MASK, 0);
        c[SITE_WORD] = (SITE_TAG | addr) as i64;
        c[HOLDER_STATE] |= STATE_CLASS_SITE;
        &mut *new
    } else {
        &mut *((c[SITE_WORD] as u64 & crate::value::POINTER_MASK) as usize as *mut Site)
    };
    let token = (u64::from(object_shape_stamp(recv)) | PIC_ID_TOKEN_BIT) as i64;
    let index = s
        .entries
        .iter()
        .position(|e| e.token == token && e.class_id == (*recv).class_id)
        .unwrap_or_else(|| {
            let i = s.next;
            s.next = (s.next + 1) % WAYS;
            i
        });
    s.entries[index] = Entry {
        token,
        class_id: (*recv).class_id,
        depth: w.depth as u8,
        absent: w.slot.is_none(),
        slot: w.slot.unwrap_or(0),
        holder: w.holder,
        holder_shape: w.holder_shape,
        hops: w.hops,
    };
    PRIMES.fetch_add(1, Ordering::Relaxed);
    super::super::stats_report_enabled();
}

/// The PIC arena retains every site; its class entries are strong roots
/// until workers start. Every address is rewritten in place after evacuation.
pub(super) fn scan_roots(c: &mut PicCache, visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let word = c[SITE_WORD] as u64;
    if c[HOLDER_STATE] & STATE_CLASS_SITE == 0 || word & !crate::value::POINTER_MASK != SITE_TAG {
        return;
    }
    let s = unsafe { &mut *((word & crate::value::POINTER_MASK) as usize as *mut Site) };
    for e in &mut s.entries {
        if e.token == 0 {
            continue;
        }
        if visitor.visit_tagged_usize_slot(&mut e.holder, crate::value::POINTER_TAG) {
            ROOT_REWRITES.fetch_add(1, Ordering::Relaxed);
        }
        for i in 0..(e.depth as usize).saturating_sub(1) {
            if visitor.visit_tagged_usize_slot(&mut e.hops[i].0, crate::value::POINTER_TAG) {
                ROOT_REWRITES.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreign_scratch_word_is_not_a_site() {
        let mut cache = [0i64; crate::object::PIC_CACHE_WORDS];
        cache[SITE_WORD] = 0xA11CE;
        assert!(unsafe { site(&cache) }.is_none());
        cache[HOLDER_STATE] |= STATE_CLASS_SITE;
        assert!(unsafe { site(&cache) }.is_none());
    }

    #[test]
    fn bare_class_link_replacement_with_same_holder_shape_declines() {
        if !crate::object::method_site::run_with_fresh_worker_gate(
            "bare_class_link_replacement_with_same_holder_shape_declines",
        ) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        const CID: u32 = 0x0C3C_79A3;
        const PROTO_CID: u32 = 0x0C3C_79A4;
        let proto_keys =
            crate::object::js_build_class_keys_array(PROTO_CID, 1, b"marker".as_ptr(), 6, 0);
        let proto_shape = crate::object::shapes::js_object_shape_id_for_class_keys(
            proto_keys as usize as u64,
            1,
            PROTO_CID,
            0,
        );
        let a = crate::object::js_object_alloc_class_inline_keys_stamped(
            PROTO_CID,
            0,
            1,
            proto_keys,
            proto_shape,
            0,
        );
        crate::object::class_decl_prototype_object_root_store(CID, a);
        let b = crate::object::js_object_alloc_class_inline_keys_stamped(
            PROTO_CID,
            0,
            1,
            proto_keys,
            proto_shape,
            0,
        );
        let a = crate::object::class_decl_prototype_object(CID);
        assert_ne!(a, b);
        assert_eq!(unsafe { object_shape_stamp(a) }, unsafe {
            object_shape_stamp(b)
        });
        let recv_keys = crate::object::js_build_class_keys_array(CID, 1, b"own".as_ptr(), 3, 0);
        let recv_shape = crate::object::shapes::js_object_shape_id_for_class_keys(
            recv_keys as usize as u64,
            1,
            CID,
            0,
        );
        let recv = crate::object::js_object_alloc_class_inline_keys_stamped(
            CID, 0, 1, recv_keys, recv_shape, 0,
        );
        assert_eq!(unsafe { class_link(recv) }, Some(a as *const ObjectHeader));
        let entry = Entry {
            token: (PIC_ID_TOKEN_BIT | u64::from(recv_shape)) as i64,
            class_id: CID,
            depth: 1,
            absent: true,
            slot: 0,
            holder: a as usize,
            holder_shape: proto_shape,
            hops: [(0, 0); HOLDER_MAX_DEPTH - 1],
        };
        assert_eq!(
            unsafe { answer(&entry, recv) },
            Some(crate::value::TAG_UNDEFINED)
        );
        let mut data_entry = entry;
        data_entry.absent = false;
        unsafe {
            let slot = (a as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut u64;
            // GC_STORE_AUDIT(POINTER_FREE): the test stores Number bits, never a heap pointer.
            std::ptr::write(slot, 42.0f64.to_bits());
            assert_eq!(answer(&data_entry, recv), Some(42.0f64.to_bits()));
            // GC_STORE_AUDIT(POINTER_FREE): undefined is an immediate NaN-box tag.
            std::ptr::write(slot, crate::value::TAG_UNDEFINED);
            assert_eq!(answer(&data_entry, recv), None);
            // GC_STORE_AUDIT(POINTER_FREE): null is an immediate NaN-box tag.
            std::ptr::write(slot, crate::value::TAG_NULL);
            assert_eq!(answer(&data_entry, recv), None);
            // GC_STORE_AUDIT(POINTER_FREE): the test stores Number bits, never a heap pointer.
            std::ptr::write(slot, 43.0f64.to_bits());
            assert_eq!(answer(&data_entry, recv), Some(43.0f64.to_bits()));
        }
        crate::object::class_decl_prototype_object_root_store(CID, b);
        assert_eq!(unsafe { answer(&entry, recv) }, None);

        crate::object::class_decl_prototype_object_root_store(CID, a);
        let record = Box::into_raw(Box::new(Site {
            entries: [entry; WAYS],
            next: 0,
        }));
        let mut cache = [0i64; crate::object::PIC_CACHE_WORDS];
        cache[SITE_WORD] = (SITE_TAG | record as usize as u64) as i64;
        cache[HOLDER_STATE] = STATE_CLASS_SITE;
        let mut slot = &mut cache as *mut PicCache;
        assert_eq!(
            unsafe { try_hit(recv, &mut slot) }.map(|v| v.bits()),
            Some(crate::value::TAG_UNDEFINED)
        );
        WORKER_AGENTS_EXIST.store(1, Ordering::SeqCst);
        assert!(unsafe { try_hit(recv, &mut slot) }.is_none());
        unsafe { drop(Box::from_raw(record)) };
    }
}
