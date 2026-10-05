//! Bounded read-site facts for declared-class instances.
//!
//! A bare CLASS ShapeId identifies the receiver's own keys but does not pin
//! `C.prototype`: the class registry can replace that pointer without a
//! receiver restamp. Every writer that can replace it bumps the class
//! lookup-surface generation (`class_registry::class_lookup_surface_gen_bump`),
//! so an entry records the generation under which its direct link was last
//! proved. While the generation is unchanged the link is the recorded holder
//! (or first hop) and the GC-leaf read front answers the entry
//! ([`leaf_answer`]) from shape words alone, as it answers the site's own
//! holder entry. A changed generation declines there; the collecting miss arm
//! re-reads the live direct prototype, and a match re-proves the entry under
//! the new generation.
//! The entries belong to one PicCache site (word 2 points to its bounded
//! process-lifetime record), never to a process-global `(shape, key)` table.
//!
//! A class hierarchy is often deeper than the site's own holder entry can
//! name (babel's parser: a method on `Tokenizer.prototype` read through eight
//! prototypes of mixin and subclass layers). An entry therefore describes up
//! to [`CLASS_READ_MAX_DEPTH`] objects. Its intermediate hops live in one
//! block, sized to the chain, that the entry owns. Every hop is still
//! compared by ShapeId on every use; depth adds compares, never a different
//! kind of fact.

use super::*;

const SITE_WORD: usize = 2; // the existing PIC's unused scratch word
const SITE_TAG: u64 = 0xA2C1_0000_0000_0000;
const STATE_CLASS_SITE: i64 = 4;
/// The site's class entries refused a receiver's chain before its read (too
/// deep, a hop that is not admitted, an accessor): no class prime from here
/// on. The holder entry's own latch
/// (`STATE_LATCHED`) does not stop class primes.
const STATE_CLASS_LATCHED: i64 = 8;
const _: () = assert!(STATE_CLASS_LATCHED < 1 << STATE_REPRIME_SHIFT);
const _: () =
    assert!(STATE_CLASS_LATCHED & (STATE_CLASS_SITE | STATE_LATCHED | STATE_REGISTERED) == 0);
const WAYS: usize = 16;

/// The most objects a class entry's chain may span, the holder (or the
/// terminal object of an absent read) included.
pub(super) const CLASS_READ_MAX_DEPTH: usize = 16;

#[derive(Clone, Copy)]
struct Entry {
    token: i64,
    class_id: u32,
    depth: u8,
    absent: bool,
    /// A holder slot word (`HOLDER_SLOT_SPILL` for a spill position).
    slot: u32,
    holder: usize,
    holder_shape: u32,
    /// The entry's `depth - 1` intermediate hops, receiver side first: a
    /// block the entry alone owns (null at depth 1). Allocated when the entry
    /// is published, reused or freed when its way is overwritten.
    hops: *mut Hop,
    /// The class lookup-surface generation under which the receiver's direct
    /// link was last proved to be `holder` (depth 1) or `hops[0]`.
    generation: u64,
    /// Every hop's ShapeId records a prototype identity that pins its next
    /// object (the realm's `%Object.prototype%`, null, or a serial), so a
    /// matching hop ShapeId proves the link without re-reading it.
    pinned_hops: bool,
}

const EMPTY: Entry = Entry {
    token: 0,
    class_id: 0,
    depth: 0,
    absent: false,
    slot: 0,
    holder: 0,
    holder_shape: 0,
    hops: std::ptr::null_mut(),
    generation: 0,
    pinned_hops: false,
};

impl Entry {
    /// The entry's intermediate hops.
    #[inline(always)]
    unsafe fn hops(&self) -> &[Hop] {
        hop_block(self.hops, self.depth)
    }
}

/// The hop block of an entry of `depth` objects.
#[inline(always)]
unsafe fn hop_block<'a>(hops: *mut Hop, depth: u8) -> &'a mut [Hop] {
    let len = (depth as usize).saturating_sub(1);
    if len == 0 {
        &mut []
    } else {
        std::slice::from_raw_parts_mut(hops, len)
    }
}

struct Site {
    entries: [Entry; WAYS],
    next: usize,
    accessor_hops: [(usize, u32); HOLDER_MAX_DEPTH - 1],
    accessor_depth: usize,
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

/// Does a hop whose ShapeId records `pid` link to one fixed object for as
/// long as the ShapeId matches? The realm's `%Object.prototype%`, null, a
/// serial, and a MIXED identity (a class id plus the serial of the explicit
/// prototype object) do: a serial names one prototype object, and any
/// `setPrototypeOf` on the hop moves its ShapeId. A bare CLASS identity
/// resolves through the class registry and is not pinned by the hop's shape.
fn hop_identity_pins_link(pid: u64) -> bool {
    pid == PROTO_ID_DEFAULT
        || pid == PROTO_ID_NULL
        || pid < crate::object::shapes::PROTO_ID_CLASS
        || (PROTO_ID_MIXED..PROTO_ID_UNIQUE).contains(&pid)
}

/// The current link from an intermediate hop, computed by the same admitted
/// shape/prototype rule the prime walk used. A changed or exotic link declines.
unsafe fn admitted_next(hop: *const ObjectHeader) -> Option<usize> {
    let pid = shape_proto_id(object_shape_stamp(hop))?;
    let (stated, word) = super::stated_link(hop);
    if stated != pid {
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
        super::next_from_word(hop, word) as usize
    };
    (next != 0).then_some(next)
}

unsafe fn answer(e: &mut Entry, recv: *const ObjectHeader) -> Option<u64> {
    if e.token == 0
        || e.class_id != (*recv).class_id
        || e.token != (u64::from(object_shape_stamp(recv)) | PIC_ID_TOKEN_BIT) as i64
    {
        return None;
    }
    let generation = crate::object::class_lookup_surface_generation();
    if e.generation != generation {
        let direct = if e.depth == 1 { e.holder } else { (*e.hops).0 };
        if class_link(recv)? as usize != direct {
            return None;
        }
        e.generation = generation;
    }
    if e.pinned_hops {
        return pinned_answer(e);
    }
    let mut previous = 0usize;
    for (i, &(addr, shape)) in e.hops().iter().enumerate() {
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
    value_of(e)
}

/// The entry's answer once the receiver and its direct link are proved: the
/// hops and the holder by ShapeId alone (the entry's hops all record pinning
/// identities), as `entry_answer_other` proves a site's own deep entry.
// Keep this in the leaf caller: outlining it makes that caller preserve
// extra registers even when an ordinary data holder answers before this arm.
#[inline(always)]
unsafe fn pinned_answer(e: &Entry) -> Option<u64> {
    for &(addr, shape) in e.hops() {
        if addr == 0 || shape_word(addr) != shape {
            return None;
        }
    }
    if e.holder == 0 || shape_word(e.holder) != e.holder_shape {
        return None;
    }
    value_of(e)
}

#[inline]
unsafe fn value_of(e: &Entry) -> Option<u64> {
    if e.absent {
        return Some(crate::value::TAG_UNDEFINED);
    }
    let bits = holder_slot_value(e.holder, e.slot)?;
    // The generic inherited getter treats nullish/hole holder values as
    // a miss and may continue to a farther prototype. The holder's
    // ShapeId does not change on a value overwrite, so recheck each hit.
    (bits != crate::value::TAG_UNDEFINED
        && bits != crate::value::TAG_NULL
        && bits != crate::value::TAG_HOLE)
        .then_some(bits)
}

/// The site's class entry for `recv` from the GC-leaf read front: loads and
/// compares only. Answers only an entry whose generation is current (the
/// direct link is the recorded one) and whose hops are pinned by their
/// ShapeIds; anything else declines (`None`) to the collecting miss arm.
///
/// # Safety
/// `c` is a live site cache; `recv` an object whose ShapeId the caller read
/// as `token`'s.
#[inline]
pub(super) unsafe fn leaf_answer(
    c: &PicCache,
    recv: *const ObjectHeader,
    token: i64,
) -> Option<u64> {
    let s = site(c)?;
    let generation = crate::object::class_lookup_surface_generation();
    let class_id = (*recv).class_id;
    for e in &s.entries {
        if e.token == token && e.class_id == class_id && e.generation == generation && e.pinned_hops
        {
            return pinned_answer(e);
        }
    }
    None
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
    let s = site(&*cache)? as *const Site as *mut Site;
    for e in &mut (*s).entries {
        if let Some(bits) = answer(e, recv) {
            HITS.fetch_add(1, Ordering::Relaxed);
            super::super::stats_report_enabled();
            return Some(crate::value::JSValue::from_bits(bits));
        }
    }
    None
}

/// May the site prime a class entry? (Its class entries have not latched.)
#[inline]
pub(super) fn may_prime(c: &PicCache) -> bool {
    c[HOLDER_STATE] & STATE_CLASS_LATCHED == 0
}

/// Latch the site's class entries (when it has a cache).
#[inline]
unsafe fn latch(cache: *mut PicCache) {
    if !cache.is_null() {
        (*cache)[HOLDER_STATE] |= STATE_CLASS_LATCHED;
    }
}

/// Prime only after the generic getter's result has been compared with the
/// same live chain and slot. Returns None if this receiver has no class link.
pub(super) unsafe fn prime(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
    cache_slot: *mut PicCacheSlot,
    name: &[u8],
) -> Option<crate::value::JSValue> {
    if class_link(obj).is_none() || !holder_name_admitted(name) {
        return None;
    }
    let existing = crate::object::field_get_set::pic_slot_peek::<PicCache>(cache_slot);
    if !existing.is_null() && !may_prime(&*existing) {
        return None;
    }
    if key_may_be_accessor(obj, name) || walk_to(obj, name, true, CLASS_READ_MAX_DEPTH).is_none() {
        latch(existing);
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
        let Some(w) = walk_to(obj, name, true, CLASS_READ_MAX_DEPTH) else {
            return Some(value);
        };
        let bits = value.bits();
        let confirmed = match w.slot {
            None => bits == crate::value::TAG_UNDEFINED,
            Some(slot) => {
                holder_slot_value(w.holder, slot) == Some(bits)
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

unsafe fn site_mut(cache: *mut PicCache) -> &'static mut Site {
    let c = &mut *cache;
    // Other PIC users may leave scratch data in word 2. Only a marker that
    // we published together with a tagged pointer grants dereference rights.
    let tagged = c[SITE_WORD] as u64;
    let has_site =
        c[HOLDER_STATE] & STATE_CLASS_SITE != 0 && tagged & !crate::value::POINTER_MASK == SITE_TAG;
    if !has_site {
        let new = Box::into_raw(Box::new(Site {
            entries: [EMPTY; WAYS],
            next: 0,
            accessor_hops: [(0, 0); HOLDER_MAX_DEPTH - 1],
            accessor_depth: 0,
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
    }
}

pub(super) unsafe fn clear_accessor_hops(c: &PicCache) {
    let word = c[SITE_WORD] as u64;
    if c[HOLDER_STATE] & STATE_CLASS_SITE == 0 || word & !crate::value::POINTER_MASK != SITE_TAG {
        return;
    }
    let s = (word & crate::value::POINTER_MASK) as usize as *mut Site;
    (*s).accessor_depth = 0;
    (*s).accessor_hops = [(0, 0); HOLDER_MAX_DEPTH - 1];
}

pub(super) unsafe fn publish_accessor_hops(
    cache: *mut PicCache,
    hops: &[(usize, u32); HOLDER_MAX_DEPTH - 1],
    depth: usize,
) {
    let s = site_mut(cache);
    s.accessor_hops = *hops;
    s.accessor_depth = depth;
}

pub(super) unsafe fn accessor_hops_match(c: &PicCache) -> bool {
    let Some(s) = site(c) else {
        return false;
    };
    s.accessor_depth != 0
        && s.accessor_hops[..s.accessor_depth]
            .iter()
            .all(|&(addr, shape)| addr != 0 && shape_word(addr) == shape)
}

unsafe fn publish(cache: *mut PicCache, recv: *const ObjectHeader, w: &Walk) {
    let s = site_mut(cache);
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
    let walked = &w.hops[..w.depth.saturating_sub(1)];
    let pinned_hops = walked
        .iter()
        .all(|&(_, shape)| shape_proto_id(shape).is_some_and(hop_identity_pins_link));
    // The way's previous block is reused for a chain of the same depth and
    // freed otherwise; the entry written below is its only owner.
    let old = &s.entries[index];
    let mut hops = old.hops;
    if old.hops().len() != walked.len() {
        if !hops.is_null() {
            drop(Box::from_raw(hop_block(hops, old.depth) as *mut [Hop]));
        }
        hops = if walked.is_empty() {
            std::ptr::null_mut()
        } else {
            Box::into_raw(Box::<[Hop]>::from(walked)) as *mut Hop
        };
    } else {
        hop_block(hops, w.depth as u8).copy_from_slice(walked);
    }
    s.entries[index] = Entry {
        token,
        class_id: (*recv).class_id,
        depth: w.depth as u8,
        absent: w.slot.is_none(),
        slot: w.slot.unwrap_or(0),
        holder: w.holder,
        holder_shape: w.holder_shape,
        hops,
        generation: crate::object::class_lookup_surface_generation(),
        pinned_hops,
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
    for (addr, _) in &mut s.accessor_hops[..s.accessor_depth] {
        visitor.visit_tagged_usize_slot(addr, crate::value::POINTER_TAG);
    }
    for e in &mut s.entries {
        if e.token == 0 {
            continue;
        }
        if visitor.visit_tagged_usize_slot(&mut e.holder, crate::value::POINTER_TAG) {
            ROOT_REWRITES.fetch_add(1, Ordering::Relaxed);
        }
        for hop in unsafe { hop_block(e.hops, e.depth) } {
            if visitor.visit_tagged_usize_slot(&mut hop.0, crate::value::POINTER_TAG) {
                ROOT_REWRITES.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fake objects whose only meaningful word is their ShapeId.
    fn shaped(shape: u32) -> Box<ObjectHeader> {
        Box::new(ObjectHeader {
            class_id: 0,
            parent_class_id: shape,
            meta: std::ptr::null_mut(),
        })
    }

    fn deep_walk(chain: &[Box<ObjectHeader>], holder: &ObjectHeader) -> Walk {
        let mut hops = NO_HOPS;
        for (i, h) in chain.iter().enumerate() {
            hops[i] = ((&**h as *const ObjectHeader) as usize, h.parent_class_id);
        }
        Walk {
            holder: (holder as *const ObjectHeader) as usize,
            holder_shape: holder.parent_class_id,
            slot: None,
            hops,
            depth: chain.len() + 1,
            getter: 0,
        }
    }

    /// A chain deeper than the site's own holder words: every hop, the ones
    /// past the holder entry's words included, is compared on every use, the
    /// root scan reaches the deepest hop, and a way overwritten by a shallow
    /// chain trades its block for one sized to the new chain.
    #[test]
    fn deep_entry_compares_every_hop() {
        if !crate::object::method_site::run_with_fresh_worker_gate("deep_entry_compares_every_hop")
        {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        let base = crate::object::shapes::SHAPE_ID_BASE;
        let recv = shaped(base + 1);
        let chain: Vec<Box<ObjectHeader>> = (0..CLASS_READ_MAX_DEPTH as u32 - 1)
            .map(|i| shaped(base + 10 + i))
            .collect();
        let holder = shaped(base + 200);
        let w = deep_walk(&chain, &holder);
        assert_eq!(w.depth, CLASS_READ_MAX_DEPTH);
        let mut cache = [0i64; crate::object::PIC_CACHE_WORDS];
        // Skip registration: the stack cache is not a process-lifetime PIC
        // allocation, and this test drives only the published words.
        cache[HOLDER_STATE] = STATE_REGISTERED;
        unsafe { publish(&mut cache, &*recv, &w) };
        let token = (PIC_ID_TOKEN_BIT | u64::from(base + 1)) as i64;
        let s = unsafe { site(&cache) }.expect("published site");
        let e = s.entries.iter().find(|e| e.token == token).expect("entry");
        assert_eq!(
            unsafe { e.hops() }.len(),
            CLASS_READ_MAX_DEPTH - 1,
            "a deep chain's block holds every hop"
        );
        // These fake hops carry no registered prototype identity; the hop
        // compares below are what a pinned entry's answer runs.
        let mut e = *e;
        e.pinned_hops = true;
        e.generation = crate::object::class_lookup_surface_generation();
        assert_eq!(
            unsafe { pinned_answer(&e) },
            Some(crate::value::TAG_UNDEFINED)
        );
        // Each hop's ShapeId is a fact the answer rests on, the deepest too.
        for i in [
            0,
            HOLDER_MAX_DEPTH - 2,
            HOLDER_MAX_DEPTH - 1,
            CLASS_READ_MAX_DEPTH - 2,
        ] {
            let hop = &*chain[i] as *const ObjectHeader as *mut ObjectHeader;
            unsafe { (*hop).parent_class_id = base + 300 };
            assert_eq!(unsafe { pinned_answer(&e) }, None, "hop {i} moved");
            unsafe { (*hop).parent_class_id = base + 10 + i as u32 };
        }
        // The root scan visits every hop, the deepest too.
        let deepest = (&*chain[CLASS_READ_MAX_DEPTH - 2] as *const ObjectHeader) as usize;
        let mut seen: Vec<u64> = Vec::new();
        {
            let mut mark = |v: f64| seen.push(v.to_bits() & crate::value::POINTER_MASK);
            let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(&mut mark);
            scan_roots(&mut cache, &mut visitor);
        }
        assert!(
            seen.contains(&(deepest as u64)),
            "the root scan must visit the deepest hop"
        );
        // A shallow chain published over the same way gets a block its size.
        let shallow = deep_walk(&chain[..1], &holder);
        unsafe { publish(&mut cache, &*recv, &shallow) };
        let s = unsafe { site(&cache) }.expect("published site");
        let e = s.entries.iter().find(|e| e.token == token).expect("entry");
        assert_eq!(e.depth, 2);
        assert_eq!(unsafe { e.hops() }, &[w.hops[0]][..]);
        let block = unsafe { hop_block(e.hops, e.depth) } as *mut [Hop];
        unsafe { drop(Box::from_raw(block)) };
        let record = (cache[SITE_WORD] as u64 & crate::value::POINTER_MASK) as usize as *mut Site;
        unsafe { drop(Box::from_raw(record)) };
    }

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
        let mut entry = Entry {
            token: (PIC_ID_TOKEN_BIT | u64::from(recv_shape)) as i64,
            class_id: CID,
            depth: 1,
            absent: true,
            slot: 0,
            holder: a as usize,
            holder_shape: proto_shape,
            hops: std::ptr::null_mut(),
            generation: 0,
            pinned_hops: true,
        };
        assert_eq!(
            unsafe { answer(&mut entry, recv) },
            Some(crate::value::TAG_UNDEFINED)
        );
        let mut data_entry = entry;
        data_entry.absent = false;
        unsafe {
            let slot = (a as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut u64;
            // GC_STORE_AUDIT(POINTER_FREE): the test stores Number bits, never a heap pointer.
            std::ptr::write(slot, 42.0f64.to_bits());
            assert_eq!(answer(&mut data_entry, recv), Some(42.0f64.to_bits()));
            // GC_STORE_AUDIT(POINTER_FREE): undefined is an immediate NaN-box tag.
            std::ptr::write(slot, crate::value::TAG_UNDEFINED);
            assert_eq!(answer(&mut data_entry, recv), None);
            // GC_STORE_AUDIT(POINTER_FREE): null is an immediate NaN-box tag.
            std::ptr::write(slot, crate::value::TAG_NULL);
            assert_eq!(answer(&mut data_entry, recv), None);
            // GC_STORE_AUDIT(POINTER_FREE): the test stores Number bits, never a heap pointer.
            std::ptr::write(slot, 43.0f64.to_bits());
            assert_eq!(answer(&mut data_entry, recv), Some(43.0f64.to_bits()));
        }
        crate::object::class_decl_prototype_object_root_store(CID, b);
        assert_eq!(unsafe { answer(&mut entry, recv) }, None);
        // The displaced holder's ShapeId was retired: an entry naming it can
        // never answer again, whatever the registry says later.
        assert_ne!(unsafe { object_shape_stamp(a) }, proto_shape);

        crate::object::class_decl_prototype_object_root_store(CID, a);
        assert_eq!(unsafe { answer(&mut entry, recv) }, None);
        let mut entry = Entry {
            holder_shape: unsafe { object_shape_stamp(a) },
            ..entry
        };
        assert_eq!(
            unsafe { answer(&mut entry, recv) },
            Some(crate::value::TAG_UNDEFINED)
        );
        let record = Box::into_raw(Box::new(Site {
            entries: [entry; WAYS],
            next: 0,
            accessor_hops: [(0, 0); HOLDER_MAX_DEPTH - 1],
            accessor_depth: 0,
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
