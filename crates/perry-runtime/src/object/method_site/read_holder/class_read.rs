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
const WAYS: usize = 16;
// The cursor's low bits select the next way; its high bits name ways with
// receiver sets. A primary-token hit pays no receiver-set lookup overhead.
const CURSOR_BITS: u32 = WAYS.trailing_zeros();
const CURSOR_MASK: usize = WAYS - 1;
const SHARED_MASK: usize = (1 << WAYS) - 1;
const ABSENT_CHURN: usize = 1 << (CURSOR_BITS + WAYS as u32);
const _: () = assert!(WAYS.is_power_of_two() && CURSOR_BITS + (WAYS as u32) < usize::BITS);
/// An absent entry has no slot to load. Its slot word can instead describe
/// a bounded set of receiver ShapeIds sharing the SAME chain and prototype
/// identity. The ids occupy the tail of its owned hop block; they are not
/// object addresses and the root scan visits only the chain prefix.
const MULTI_ABSENT: u32 = 1 << 31;
const ABSENT_BUCKETS: usize = 256;
const ABSENT_WORDS: usize =
    ABSENT_BUCKETS * std::mem::size_of::<u32>() / std::mem::size_of::<Hop>();
const ABSENT_RECEIVERS: u32 = 128; // at most half full, bounded probes
const _: () = assert!(ABSENT_BUCKETS.is_power_of_two());
const _: () = assert!(ABSENT_RECEIVERS < ABSENT_BUCKETS as u32);
const _: () = assert!(
    ABSENT_WORDS * std::mem::size_of::<Hop>() == ABSENT_BUCKETS * std::mem::size_of::<u32>()
);

/// The most objects a class entry's chain may span, the holder (or the
/// terminal object of an absent read) included.
pub(super) const CLASS_READ_MAX_DEPTH: usize = 16;

#[derive(Clone, Copy)]
struct Entry {
    token: i64,
    class_id: u32,
    depth: u8,
    absent: bool,
    pinned_hops: bool,
    /// A holder slot word (`HOLDER_SLOT_SPILL` for a spill position), or
    /// `MULTI_ABSENT | receiver_count` for a shared absent proof.
    slot: u32,
    holder: usize,
    holder_shape: u32,
    /// The entry's `depth - 1` intermediate hops, receiver side first: a
    /// block the entry alone owns (null for a single-shape depth-1 entry).
    /// A multi-shape absent proof appends `ABSENT_BUCKETS` u32 ids, outside
    /// the chain prefix the root scan visits. Reused or freed on overwrite.
    hops: *mut Hop,
    /// The class lookup-surface generation under which the receiver's direct
    /// link was last proved to be `holder` (depth 1) or `hops[0]`.
    generation: u64,
}

const EMPTY: Entry = Entry {
    token: 0,
    class_id: 0,
    depth: 0,
    absent: false,
    pinned_hops: false,
    slot: 0,
    holder: 0,
    holder_shape: 0,
    hops: std::ptr::null_mut(),
    generation: 0,
};

impl Entry {
    /// Retirement invalidates the whole proof, including its terminal and
    /// intermediate shapes. Only cold publication searches query the table.
    #[cold]
    #[inline(never)]
    unsafe fn retired(&self) -> bool {
        let retired = crate::object::shapes::shape_is_retired;
        retired(self.token as u32)
            || (self.holder != 0 && retired(self.holder_shape))
            || self.hops().iter().any(|&(_, shape)| retired(shape))
    }
    /// The entry's intermediate hops, excluding receiver ids in its tail.
    #[inline(always)]
    unsafe fn hops(&self) -> &[Hop] {
        hop_block(self.hops, self.depth)
    }

    #[inline]
    fn multi_absent(&self) -> bool {
        self.absent && self.slot & MULTI_ABSENT != 0
    }

    fn block_len(&self) -> usize {
        (self.depth as usize).saturating_sub(1) + if self.multi_absent() { ABSENT_WORDS } else { 0 }
    }

    /// The whole allocation, used only by its owner when replacing a way.
    #[optimize(size)]
    unsafe fn drop_block(&self) {
        if !self.hops.is_null() {
            drop(Box::from_raw(
                std::slice::from_raw_parts_mut(self.hops, self.block_len()) as *mut [Hop],
            ));
        }
    }

    /// Open-addressed receiver facts, never GC pointers. A zero id is empty.
    #[optimize(size)]
    #[inline(always)]
    unsafe fn receiver_bucket(&self, shape: u32) -> *mut u32 {
        let tail = self.hops.add(self.depth as usize - 1).cast::<u32>();
        let mut i = shape as usize & (ABSENT_BUCKETS - 1);
        loop {
            let bucket = tail.add(i);
            if *bucket == 0 || *bucket == shape {
                return bucket;
            }
            i = (i + 1) & (ABSENT_BUCKETS - 1);
        }
    }

    #[inline(always)]
    unsafe fn matches_receiver(&self, token: i64, class_id: u32) -> bool {
        self.class_id == class_id
            && (self.token == token
                || (self.multi_absent() && *self.receiver_bucket(token as u32) != 0))
    }

    /// Extend the shared proof after the generic read confirmed this chain.
    /// More receiver shapes than the bound use another ordinary class way.
    #[cold]
    #[optimize(size)]
    #[inline(never)]
    unsafe fn add_receiver(&mut self, shape: u32) -> bool {
        if !self.multi_absent() {
            let chain_len = self.depth as usize - 1;
            let block = Box::<[Hop]>::new_uninit_slice(chain_len + ABSENT_WORDS);
            let hops = Box::into_raw(block) as *mut Hop;
            if chain_len != 0 {
                std::ptr::copy_nonoverlapping(self.hops, hops, chain_len);
            }
            // Initialize the entire tail, including Hop padding: this region
            // is u32 receiver ids, never interpreted or traced as hops.
            std::ptr::write_bytes(hops.add(chain_len).cast::<u32>(), 0, ABSENT_BUCKETS);
            self.drop_block();
            self.hops = hops;
            self.slot = MULTI_ABSENT;
            *self.receiver_bucket(self.token as u32) = self.token as u32;
            self.slot += 1;
        }
        let mut bucket = self.receiver_bucket(shape);
        if *bucket != 0 {
            return true;
        }
        if self.slot & !MULTI_ABSENT == ABSENT_RECEIVERS {
            // ShapeIds are never reused. At capacity, discard expired facts
            // through the shape table itself, then rehash: clearing individual
            // buckets would break the probe chains of surviving receivers.
            let tail = self.hops.add(self.depth as usize - 1).cast::<u32>();
            let mut live = [0u32; ABSENT_RECEIVERS as usize];
            let mut count = 0;
            for i in 0..ABSENT_BUCKETS {
                let id = *tail.add(i);
                if id != 0 && !crate::object::shapes::shape_is_retired(id) {
                    live[count] = id;
                    count += 1;
                }
            }
            if count == ABSENT_RECEIVERS as usize {
                return false;
            }
            std::ptr::write_bytes(tail, 0, ABSENT_BUCKETS);
            self.slot = MULTI_ABSENT | count as u32;
            for &id in &live[..count] {
                *self.receiver_bucket(id) = id;
            }
            bucket = self.receiver_bucket(shape);
        }
        *bucket = shape;
        self.slot += 1;
        true
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

#[repr(C)]
struct Site {
    // Match main's first class entry without loading vector bounds or keeping
    // a polymorphic loop live in the read front. Further entries are lazy.
    primary_class: Entry,
    entries: Vec<Entry>,
    next: usize,
    holders: Vec<HolderEntry>,
    holder_next: usize,
    accessor_hops: [(usize, u32); HOLDER_MAX_DEPTH - 1],
    accessor_depth: usize,
}

impl Site {
    fn class_entry(&self, index: usize) -> &Entry {
        if index == 0 {
            &self.primary_class
        } else {
            &self.entries[index - 1]
        }
    }

    fn class_entries(&self) -> impl Iterator<Item = &Entry> {
        std::iter::once(&self.primary_class).chain(self.entries.iter())
    }

    fn class_entries_mut(&mut self) -> impl Iterator<Item = &mut Entry> {
        std::iter::once(&mut self.primary_class).chain(self.entries.iter_mut())
    }

    fn class_entry_mut(&mut self, index: usize) -> &mut Entry {
        if index == 0 {
            &mut self.primary_class
        } else {
            &mut self.entries[index - 1]
        }
    }
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

#[inline]
pub(super) fn has_site(c: &PicCache) -> bool {
    c[HOLDER_STATE] & STATE_CLASS_SITE != 0
}

#[inline(always)]
unsafe fn answer(e: &mut Entry, recv: *const ObjectHeader) -> Option<u64> {
    if e.token == 0
        || !e.matches_receiver(
            (u64::from(object_shape_stamp(recv)) | PIC_ID_TOKEN_BIT) as i64,
            (*recv).class_id,
        )
    {
        return None;
    }
    if !reprove_direct(e, recv) {
        return None;
    }
    pinned_answer(e)
}

// Both collecting paths re-prove the receiver's direct class link in one
// place. Shape-only leaf hits decline rather than consulting the registry.
#[inline]
unsafe fn reprove_direct(e: &mut Entry, recv: *const ObjectHeader) -> bool {
    let generation = crate::object::class_lookup_surface_generation();
    if e.generation != generation {
        let direct = if e.depth == 1 { e.holder } else { (*e.hops).0 };
        if class_link(recv).map(|link| link as usize) != Some(direct) {
            return false;
        }
        e.generation = generation;
    }
    true
}

/// The entry's answer once the receiver and its direct link are proved: the
/// hops and the holder by ShapeId alone (the entry's hops all record pinning
/// identities), as `entry_answer_other` proves a site's own deep entry.
#[inline(always)]
unsafe fn pinned_answer(e: &Entry) -> Option<u64> {
    if !valid_chain(e) {
        return None;
    }
    value_of(e)
}

#[inline(always)]
unsafe fn valid_chain(e: &Entry) -> bool {
    for &(addr, shape) in e.hops() {
        if addr == 0 || shape_word(addr) != shape {
            return false;
        }
    }
    if e.holder == 0 || shape_word(e.holder) != e.holder_shape {
        return false;
    }
    true
}

#[inline(always)]
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
#[cfg(test)]
#[inline(always)]
pub(super) unsafe fn leaf_answer(
    c: &PicCache,
    recv: *const ObjectHeader,
    token: i64,
) -> Option<u64> {
    let bits = leaf_bits(c, recv, token);
    (bits != crate::value::TAG_HOLE).then_some(bits)
}

/// The front's existing hole sentinel avoids keeping a separate failure
/// value live across the cold scan. This shares all entry validation above.
#[inline(always)]
pub(super) unsafe fn leaf_bits(c: &PicCache, recv: *const ObjectHeader, token: i64) -> u64 {
    let Some(s) = site(c) else {
        return crate::value::TAG_HOLE;
    };
    leaf_site_bits(s, recv, token)
}

/// Site admission stays in the front; complete class-chain validation is
/// shared by the inline and outlined fronts after that admission succeeds.
#[cold]
#[inline(never)]
unsafe fn leaf_site_bits(s: &Site, recv: *const ObjectHeader, token: i64) -> u64 {
    let generation = crate::object::class_lookup_surface_generation();
    let class_id = (*recv).class_id;
    let e = &s.primary_class;
    if e.token == token && e.class_id == class_id && e.generation == generation && e.pinned_hops {
        return pinned_answer(e).unwrap_or(crate::value::TAG_HOLE);
    }
    if s.entries.is_empty() && s.holders.is_empty() && s.next & (SHARED_MASK << CURSOR_BITS) == 0 {
        return crate::value::TAG_HOLE;
    }
    secondary_leaf_bits(s, token, class_id, generation)
}

/// Neither a monomorphic ordinary nor a matching first class answer enters
/// this scan. Its loop bounds and scratch registers stay out of the front.
#[cold]
#[inline(never)]
unsafe fn secondary_leaf_bits(s: &Site, token: i64, class_id: u32, generation: u64) -> u64 {
    for e in &s.entries {
        if e.token == token && e.class_id == class_id && e.generation == generation && e.pinned_hops
        {
            return pinned_answer(e).unwrap_or(crate::value::TAG_HOLE);
        }
    }
    for e in &s.holders {
        if let Some(bits) = super::saved_entry_answer(e, token) {
            return bits;
        }
    }
    if s.next & (SHARED_MASK << CURSOR_BITS) != 0 {
        return probe_shared_leaf(s, class_id, token).to_bits();
    }
    crate::value::TAG_HOLE
}

/// Re-prove class links when their generation changes, or serve a saved
/// ordinary answer. Getters keep the collecting path and original receiver.
#[cold]
#[inline(never)]
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
    let token = (u64::from(object_shape_stamp(recv)) | PIC_ID_TOKEN_BIT) as i64;
    let class_id = (*recv).class_id;
    for e in (*s)
        .class_entries_mut()
        .filter(|e| e.token == token && e.class_id == class_id)
    {
        if let Some(bits) = answer(e, recv) {
            HITS.fetch_add(1, Ordering::Relaxed);
            super::super::stats_report_enabled();
            return Some(crate::value::JSValue::from_bits(bits));
        }
    }
    if let Some(bits) = try_shared_hit(s, recv) {
        HITS.fetch_add(1, Ordering::Relaxed);
        super::super::stats_report_enabled();
        return Some(crate::value::JSValue::from_bits(bits));
    }
    // Ordinary storage exists only after a second admitted receiver replaced
    // the primary answer. Class-only sites never run accessor validators.
    if (*s).holders.is_empty() {
        return None;
    }
    let token = (u64::from(object_shape_stamp(recv)) | PIC_ID_TOKEN_BIT) as i64;
    for e in &(*s).holders {
        if let Some(bits) = super::saved_entry_answer(e, token) {
            return Some(crate::value::JSValue::from_bits(bits));
        }
        if let Some((getter, pair)) = super::validated_accessor::<false>(e, token) {
            HITS_ACCESSOR.fetch_add(1, Ordering::Relaxed);
            return Some(super::invoke_getter(recv, getter, pair));
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
    if class_link(obj).is_none() || !holder_name_admitted(name) {
        return None;
    }
    if key_may_be_accessor(obj, name) || walk_to(obj, name, true, CLASS_READ_MAX_DEPTH).is_none() {
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

/// The existing site-owned record, shared by class and ordinary answers.
unsafe fn site_mut(cache: *mut PicCache) -> &'static mut Site {
    let c = &mut *cache;
    // Other PIC users may leave scratch data in word 2. Only a marker that
    // we published together with a tagged pointer grants dereference rights.
    let tagged = c[SITE_WORD] as u64;
    let has_site =
        c[HOLDER_STATE] & STATE_CLASS_SITE != 0 && tagged & !crate::value::POINTER_MASK == SITE_TAG;
    if !has_site {
        let new = empty_site();
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

unsafe fn publish(cache: *mut PicCache, recv: *const ObjectHeader, w: &Walk) {
    let walked = &w.hops[..w.depth.saturating_sub(1)];
    // Recheck the exact link admission used by walk_to, including fixed
    // declaration parents. All hits validate those same intermediate shapes.
    if !walked.iter().all(|&(holder, shape)| {
        shape_proto_id(shape).is_some_and(hop_identity_pins_link)
            || admitted_link(holder as *const ObjectHeader).is_some()
    }) {
        return;
    }
    let s = site_mut(cache);
    let token = (u64::from(object_shape_stamp(recv)) | PIC_ID_TOKEN_BIT) as i64;
    // Filling the ways alone is not evidence of churn. Arm sharing only
    // after an absent way has actually been replaced by a different shape.
    if w.slot.is_none() && s.next & ABSENT_CHURN != 0 {
        if let Some(pid) = prototype_identity(token as u32) {
            let mut group = WAYS;
            for i in 0..1 + s.entries.len() {
                // The selected group is an earlier way, disjoint from i.
                // Borrow this way in place; copying its 48-byte record needlessly
                // spills the publication loop's proof fields.
                let e = &*(s.class_entry(i) as *const Entry);
                if !same_absence(e, w, walked, (*recv).class_id, pid) {
                    continue;
                }
                // The first compatible way that can admit this shape becomes
                // the group. In the same pass retire later compatible singles,
                // after their shape ids are safely in that group's owned set.
                if group != WAYS {
                    if !e.multi_absent() && s.class_entry_mut(group).add_receiver(e.token as u32) {
                        e.drop_block();
                        *s.class_entry_mut(i) = EMPTY;
                        s.next &= !(1 << (i as u32 + CURSOR_BITS));
                    }
                } else if s.class_entry_mut(i).add_receiver(token as u32) {
                    group = i;
                }
            }
            if group != WAYS {
                s.next |= 1 << (group as u32 + CURSOR_BITS);
                return;
            }
        }
    }
    let index = publication_way(s, token, (*recv).class_id);
    s.next &= !(1 << (index as u32 + CURSOR_BITS));
    // The way's previous block is reused for a chain of the same depth and
    // freed otherwise; the entry written below is its only owner.
    let old = *s.class_entry(index);
    if old.token != 0
        && old.token != token
        && old.absent
        && w.slot.is_none()
        && old.class_id == (*recv).class_id
        && !old.retired()
    {
        s.next |= ABSENT_CHURN;
    }
    let mut hops = old.hops;
    if old.block_len() != walked.len() {
        old.drop_block();
        hops = if walked.is_empty() {
            std::ptr::null_mut()
        } else {
            Box::into_raw(Box::<[Hop]>::from(walked)) as *mut Hop
        };
    } else {
        hop_block(hops, w.depth as u8).copy_from_slice(walked);
    }
    *s.class_entry_mut(index) = Entry {
        token,
        class_id: (*recv).class_id,
        depth: w.depth as u8,
        absent: w.slot.is_none(),
        pinned_hops: true,
        slot: w.slot.unwrap_or(0),
        holder: w.holder,
        holder_shape: w.holder_shape,
        hops,
        generation: crate::object::class_lookup_surface_generation(),
    };
    PRIMES.fetch_add(1, Ordering::Relaxed);
    super::super::stats_report_enabled();
}

/// Bounded eviction of complete ordinary answers. The primary cache words
/// stay in place; only a replacement by another receiver needs this storage.
#[cold]
#[inline(never)]
pub(super) unsafe fn retain_holder(cache: *mut PicCache) {
    if (*cache)[HOLDER_RECV] == 0
        || (*cache)[HOLDER_KIND] as u64 & (HOLDER_ACCESSOR | HOLDER_ACCESSOR_DEEP)
            == (HOLDER_ACCESSOR | HOLDER_ACCESSOR_DEEP)
    {
        return;
    }
    let entry = *super::holder_words(&*cache);
    if super::holder_entry_retired(&entry) {
        return;
    }
    let s = site_mut(cache);
    let index = s
        .holders
        .iter()
        .position(|c| c[HOLDER_RECV] == entry[HOLDER_RECV] || super::holder_entry_retired(c))
        .unwrap_or_else(|| {
            if s.holders.len() < 8 {
                let i = s.holders.len();
                s.holders.push(HolderEntry([0; HOLDER_STATE - HOLDER_RECV]));
                i
            } else {
                let i = s.holder_next;
                s.holder_next = (i + 1) % 8;
                i
            }
        });
    s.holders[index] = entry;
}

#[cold]
#[inline(never)]
pub(super) unsafe fn holder_answer(c: &PicCache, token: i64) -> Option<u64> {
    let s = site(c)?;
    for entry in &s.holders {
        if let Some(bits) = super::saved_entry_answer(entry, token) {
            return Some(bits);
        }
    }
    None
}

#[cfg(test)]
pub(super) unsafe fn holder_accessor<const INLINE_ONLY: bool>(
    c: &PicCache,
    token: i64,
) -> Option<(&HolderEntry, usize, usize)> {
    let s = site(c)?;
    for entry in &s.holders {
        if let Some((getter, pair)) = super::validated_accessor::<INLINE_ONLY>(entry, token) {
            return Some((entry, getter, pair));
        }
    }
    None
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
    for entry in &mut s.holders {
        super::scan_entry_roots(entry, visitor);
    }
    for e in s.class_entries_mut() {
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
mod tests;

#[cold]
#[inline(never)]
unsafe fn probe_shared_leaf(s: &Site, class_id: u32, token: i64) -> f64 {
    let generation = crate::object::class_lookup_surface_generation();
    let mut shared = (s.next >> CURSOR_BITS) & SHARED_MASK;
    while shared != 0 {
        let i = shared.trailing_zeros() as usize;
        shared &= shared - 1;
        let e = s.class_entry(i);
        // Shared bits name only absent sets; replacement clears the bit.
        // The primary scan already handled the group's primary token.
        if e.class_id == class_id
            && e.generation == generation
            && *e.receiver_bucket(token as u32) != 0
        {
            return f64::from_bits(if valid_chain(e) {
                crate::value::TAG_UNDEFINED
            } else {
                crate::value::TAG_HOLE
            });
        }
    }
    f64::from_bits(crate::value::TAG_HOLE)
}

#[cold]
#[inline(never)]
unsafe fn try_shared_hit(s: *mut Site, recv: *const ObjectHeader) -> Option<u64> {
    let token = object_shape_stamp(recv);
    let class_id = (*recv).class_id;
    let mut shared = ((*s).next >> CURSOR_BITS) & SHARED_MASK;
    while shared != 0 {
        let i = shared.trailing_zeros() as usize;
        shared &= shared - 1;
        let e = (*s).class_entry_mut(i);
        // The bitmap admits only shared absent ways. Validate their receiver
        // set and the same direct-link and chain proofs as ordinary ways.
        if e.class_id == class_id && *e.receiver_bucket(token) != 0 {
            if reprove_direct(e, recv) && valid_chain(e) {
                return Some(crate::value::TAG_UNDEFINED);
            }
        }
    }
    None
}

#[cold]
#[inline(never)]
fn prototype_identity(shape: u32) -> Option<u64> {
    shape_proto_id(shape)
}

#[cold]
#[inline(never)]
unsafe fn same_absence(e: &Entry, w: &Walk, walked: &[Hop], class_id: u32, pid: u64) -> bool {
    e.token != 0
        && !e.retired()
        && e.absent
        && e.class_id == class_id
        && e.depth as usize == w.depth
        && e.holder == w.holder
        && e.holder_shape == w.holder_shape
        && e.generation == crate::object::class_lookup_surface_generation()
        && prototype_identity(e.token as u32) == Some(pid)
        && e.hops() == walked
}

#[cold]
#[inline(never)]
pub(super) unsafe fn clear_accessor_hops(c: &PicCache) {
    let word = c[SITE_WORD] as u64;
    if c[HOLDER_STATE] & STATE_CLASS_SITE == 0 || word & !crate::value::POINTER_MASK != SITE_TAG {
        return;
    }
    let s = (word & crate::value::POINTER_MASK) as usize as *mut Site;
    (*s).accessor_depth = 0;
    (*s).accessor_hops = [(0, 0); HOLDER_MAX_DEPTH - 1];
}

#[cold]
#[inline(never)]
pub(super) unsafe fn publish_accessor_hops(
    cache: *mut PicCache,
    hops: &[(usize, u32); HOLDER_MAX_DEPTH - 1],
    depth: usize,
) {
    let s = site_mut(cache);
    s.accessor_hops = *hops;
    s.accessor_depth = depth;
}

#[cold]
#[inline(never)]
pub(super) unsafe fn accessor_hops_match(c: &PicCache) -> bool {
    let Some(s) = site(c) else {
        return false;
    };
    s.accessor_depth != 0
        && s.accessor_hops[..s.accessor_depth]
            .iter()
            .all(|&(addr, shape)| addr != 0 && shape_word(addr) == shape)
}

#[cold]
#[inline(never)]
unsafe fn publication_way(s: &mut Site, token: i64, class_id: u32) -> usize {
    if let Some(i) = s
        .class_entries()
        .position(|e| e.token == token && e.class_id == class_id)
    {
        return i;
    }
    let mut shared = (s.next >> CURSOR_BITS) & SHARED_MASK;
    while shared != 0 {
        let i = shared.trailing_zeros() as usize;
        shared &= shared - 1;
        if s.class_entry(i).matches_receiver(token, class_id) {
            return i;
        }
    }
    // Expiry is not live-shape churn: reclaim it before bounded eviction.
    if let Some(i) = s.class_entries().position(|e| e.token == 0 || e.retired()) {
        return i;
    }
    if s.primary_class.token == 0 {
        return 0;
    }
    if s.entries.len() < WAYS - 1 {
        s.entries.push(EMPTY);
        return s.entries.len();
    }
    let i = s.next & CURSOR_MASK;
    s.next = (s.next & !CURSOR_MASK) | ((i + 1) & CURSOR_MASK);
    i
}

#[cold]
#[inline(always)]
fn empty_site() -> *mut Site {
    Box::into_raw(Box::new(Site {
        primary_class: EMPTY,
        entries: Vec::new(),
        next: 0,
        holders: Vec::new(),
        holder_next: 0,
        accessor_hops: [(0, 0); HOLDER_MAX_DEPTH - 1],
        accessor_depth: 0,
    }))
}
#[cfg(test)]
#[path = "class_read/retirement_tests.rs"]
mod retirement_tests;

/// Inspect the real site in the moving-GC witness; no production API.
#[cfg(test)]
pub(crate) unsafe fn test_retirement_snapshot(c: &PicCache) -> (usize, bool, u32) {
    let s = site(c).expect("class site was primed");
    (
        s.class_entries().filter(|e| e.token != 0).count(),
        s.next & ABSENT_CHURN != 0,
        s.class_entries()
            .filter(|e| e.multi_absent())
            .map(|e| e.slot & !MULTI_ABSENT)
            .sum(),
    )
}

#[cfg(test)]
pub(crate) unsafe fn test_shared_primary(c: &PicCache) -> u32 {
    site(c)
        .unwrap()
        .class_entries()
        .find(|e| e.multi_absent())
        .expect("shared way")
        .token as u32
}
