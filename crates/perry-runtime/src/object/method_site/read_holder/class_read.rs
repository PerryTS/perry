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
//!
//! Once a site has replaced a live absent way for another receiver shape,
//! absent ways can retain up to 128
//! receiver shapes with one identical
//! chain, terminal and receiver prototype identity. Shape membership proves
//! absence on the receiver; the same per-hop checks prove the rest. The
//! receiver ids live in the entry's allocation, not a registry or side table.
//! Different chains and different prototype identities retain separate ways.

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
                if id != 0 && crate::object::shapes::shape_record_by_id(id).is_some() {
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
    entries: [Entry; WAYS],
    next: usize,
    accessor_hops: [(usize, u32); HOLDER_MAX_DEPTH - 1],
    accessor_depth: usize,
}

const _: () = assert!(std::mem::offset_of!(Site, next) == WAYS * std::mem::size_of::<Entry>());

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

// Descriptor and TLS access is a cold publication fact. Keep one outline
// instead of copying its access sequence into each receiver/hop proof.
#[cold]
#[optimize(size)]
#[inline(never)]
fn prototype_identity(shape: u32) -> Option<u64> {
    shape_proto_id(shape)
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
// Keep this in the leaf caller: outlining it makes that caller preserve
// extra registers even when an ordinary data holder answers before this arm.
#[inline(always)]
unsafe fn pinned_answer(e: &Entry) -> Option<u64> {
    if !valid_chain(e) {
        return None;
    }
    value_of(e)
}

/// One shape proof for ordinary and shared entries; a shared entry needs
/// no holder-value load because its terminal proves an absent answer.
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
    // Same primary proof as main. The cursor reaches the next-word only
    // after all primary tokens miss, so no shared state is live on a hit.
    let mut cursor = (s as *const Site).cast::<Entry>();
    let end = cursor.add(WAYS);
    while cursor != end {
        let e = &*cursor;
        if e.token == token && e.class_id == class_id && e.generation == generation && e.pinned_hops
        {
            return pinned_answer(e);
        }
        cursor = cursor.add(1);
    }
    if (*end.cast::<usize>() >> CURSOR_BITS) & SHARED_MASK == 0 {
        return None;
    }
    let bits = probe_shared_leaf(end, class_id, token).to_bits();
    (bits != crate::value::TAG_HOLE).then_some(bits)
}

// The end cursor supplies the record without keeping another pointer live
// through the primary scan. Returning the front's f64 representation lets
// its final shared edge tail-call; the Option wrapper still declines holes
// for other callers. This helper never collects.
#[cold]
#[inline(never)]
#[optimize(size)]
unsafe fn probe_shared_leaf(end: *const Entry, class_id: u32, token: i64) -> f64 {
    let s = &*end.sub(WAYS).cast::<Site>();
    let generation = crate::object::class_lookup_surface_generation();
    let mut shared = (s.next >> CURSOR_BITS) & SHARED_MASK;
    while shared != 0 {
        let i = shared.trailing_zeros() as usize;
        shared &= shared - 1;
        let e = s.entries.get_unchecked(i);
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
    let token = (u64::from(object_shape_stamp(recv)) | PIC_ID_TOKEN_BIT) as i64;
    let class_id = (*recv).class_id;
    let bits = (*s)
        .entries
        .iter_mut()
        .filter(|e| e.token == token && e.class_id == class_id)
        .find_map(|e| answer(e, recv))
        .or_else(|| try_shared_hit(s, recv))?;
    HITS.fetch_add(1, Ordering::Relaxed);
    super::super::stats_report_enabled();
    Some(crate::value::JSValue::from_bits(bits))
}

// Keep the collecting shared miss outside the ordinary receiver's hit.
#[cold]
#[optimize(size)]
#[inline(never)]
unsafe fn try_shared_hit(s: *mut Site, recv: *const ObjectHeader) -> Option<u64> {
    let token = object_shape_stamp(recv);
    let class_id = (*recv).class_id;
    let mut shared = ((*s).next >> CURSOR_BITS) & SHARED_MASK;
    while shared != 0 {
        let i = shared.trailing_zeros() as usize;
        shared &= shared - 1;
        let e = (*s).entries.get_unchecked_mut(i);
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

// All EMPTY fields have a valid all-zero representation. Assert the
// default values as well, so a future nonzero default cannot be missed.
const _: () = assert!(
    EMPTY.token == 0
        && EMPTY.class_id == 0
        && EMPTY.depth == 0
        && !EMPTY.absent
        && !EMPTY.pinned_hops
        && EMPTY.slot == 0
        && EMPTY.holder == 0
        && EMPTY.holder_shape == 0
        && EMPTY.hops.is_null()
        && EMPTY.generation == 0
);

#[cold]
#[optimize(size)]
#[inline(always)]
fn empty_site() -> *mut Site {
    // SAFETY: Site contains only the asserted zero-default entries, a usize
    // cursor and accessor hop records of zeros. Raw null pointers, false
    // bools and zero integers are all valid.
    Box::into_raw(unsafe { Box::<Site>::new_zeroed().assume_init() })
}

/// Keep the cold way searches over a slice, so constant-size empty-way
/// searches do not expand into 16 copies of the same comparison.
///
/// # Safety
/// The slice contains exactly WAYS entries, and every shared cursor bit
/// names an initialized multi-shape absent entry in that slice.
#[cold]
#[optimize(size)]
#[inline(never)]
unsafe fn publication_way(entries: &[Entry], next: &mut usize, token: i64, class_id: u32) -> usize {
    if let Some(i) = entries
        .iter()
        .position(|e| e.token == token && e.class_id == class_id)
    {
        return i;
    }
    let mut shared = (*next >> CURSOR_BITS) & SHARED_MASK;
    while shared != 0 {
        let i = shared.trailing_zeros() as usize;
        shared &= shared - 1;
        if entries.get_unchecked(i).matches_receiver(token, class_id) {
            return i;
        }
    }
    // An expired receiver can never hit again. Reuse empty or expired ways
    // before rotating over live facts, whether or not this site has churned.
    if let Some(i) = entries.iter().position(|e| {
        e.token == 0 || crate::object::shapes::shape_record_by_id(e.token as u32).is_none()
    }) {
        return i;
    }
    let i = *next & CURSOR_MASK;
    *next = (*next & !CURSOR_MASK) | ((i + 1) & CURSOR_MASK);
    i
}

#[cold]
#[optimize(size)]
#[inline(always)]
unsafe fn same_absence(e: &Entry, w: &Walk, walked: &[Hop], class_id: u32, pid: u64) -> bool {
    e.token != 0
        && e.absent
        && e.class_id == class_id
        && e.depth as usize == w.depth
        && e.holder == w.holder
        && e.holder_shape == w.holder_shape
        && e.generation == crate::object::class_lookup_surface_generation()
        && prototype_identity(e.token as u32) == Some(pid)
        && e.hops() == walked
}

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

#[optimize(size)]
#[cold]
#[inline(never)]
unsafe fn publish(cache: *mut PicCache, recv: *const ObjectHeader, w: &Walk) {
    let walked = &w.hops[..w.depth.saturating_sub(1)];
    // walk_to admits serial/default/null/MIXED identities for every
    // intermediate hop. Refuse any unproved link before publication, so
    // all hits use the same per-hop shape validation.
    if !walked
        .iter()
        .all(|&(_, shape)| shape_proto_id(shape).is_some_and(hop_identity_pins_link))
    {
        return;
    }
    let s = site_mut(cache);
    let token = (u64::from(object_shape_stamp(recv)) | PIC_ID_TOKEN_BIT) as i64;
    // Filling the ways alone is not evidence of churn. Arm sharing only
    // after an absent way has actually been replaced by a different shape.
    if w.slot.is_none() && s.next & ABSENT_CHURN != 0 {
        if let Some(pid) = prototype_identity(token as u32) {
            let mut group = WAYS;
            for i in 0..WAYS {
                // The selected group is an earlier way, disjoint from i.
                // Borrow this way in place; copying its 48-byte record needlessly
                // spills the publication loop's proof fields.
                let e = &*s.entries.as_ptr().add(i);
                if !same_absence(e, w, walked, (*recv).class_id, pid) {
                    continue;
                }
                // The first compatible way that can admit this shape becomes
                // the group. In the same pass retire later compatible singles,
                // after their shape ids are safely in that group's owned set.
                if group != WAYS {
                    if !e.multi_absent() && s.entries[group].add_receiver(e.token as u32) {
                        e.drop_block();
                        s.entries[i] = EMPTY;
                        s.next &= !(1 << (i as u32 + CURSOR_BITS));
                    }
                } else if s.entries[i].add_receiver(token as u32) {
                    group = i;
                }
            }
            if group != WAYS {
                s.next |= 1 << (group as u32 + CURSOR_BITS);
                return;
            }
        }
    }
    let index = publication_way(&s.entries, &mut s.next, token, (*recv).class_id);
    s.next &= !(1 << (index as u32 + CURSOR_BITS));
    // The way's previous block is reused for a chain of the same depth and
    // freed otherwise; the entry written below is its only owner.
    let old = &s.entries[index];
    if old.token != 0
        && old.token != token
        && old.absent
        && w.slot.is_none()
        && old.class_id == (*recv).class_id
        && crate::object::shapes::shape_record_by_id(old.token as u32).is_some()
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
    s.entries[index] = Entry {
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

    pub(super) fn pinned_shape(generation: u64) -> u32 {
        crate::object::shapes::shape_descriptor_ensure_with_generation(
            std::ptr::null(),
            0,
            0,
            generation,
            crate::object::shapes::ShapeObjectKind::Ordinary,
            PROTO_ID_DEFAULT,
            crate::object::shapes::ReceiverFacts::NONE,
        )
        .expect("pinned hop shape")
    }

    pub(super) fn receiver_shape(class_id: u32, generation: u64) -> u32 {
        crate::object::shapes::shape_descriptor_ensure_with_generation(
            std::ptr::null(),
            0,
            0,
            generation,
            crate::object::shapes::ShapeObjectKind::Ordinary,
            PROTO_ID_CLASS | u64::from(class_id),
            crate::object::shapes::ReceiverFacts::NONE,
        )
        .expect("receiver shape")
    }

    #[test]
    fn empty_site_has_no_facts_or_roots() {
        let record = empty_site();
        let mut cache = [0i64; crate::object::PIC_CACHE_WORDS];
        cache[HOLDER_STATE] = STATE_CLASS_SITE | STATE_REGISTERED;
        cache[SITE_WORD] = (SITE_TAG | record as usize as u64) as i64;
        assert_eq!(unsafe { (*record).next }, 0);
        assert!(unsafe { &(*record).entries }.iter().all(|e| e.token == 0));
        let recv = shaped(pinned_shape(39_000));
        let token = (PIC_ID_TOKEN_BIT | u64::from(recv.parent_class_id)) as i64;
        assert_eq!(unsafe { leaf_answer(&cache, &*recv, token) }, None);
        let mut seen = Vec::new();
        let mut mark = |v: f64| seen.push(v.to_bits());
        scan_roots(
            &mut cache,
            &mut crate::gc::RuntimeRootVisitor::for_copy(&mut mark),
        );
        assert!(seen.is_empty());
        for e in unsafe { &(*record).entries } {
            assert!(e.hops.is_null());
            unsafe { e.drop_block() };
        }
        unsafe { drop(Box::from_raw(record)) };
    }

    #[test]
    fn primary_ways_stay_single_until_churn() {
        if !crate::object::method_site::run_with_fresh_worker_gate(
            "primary_ways_stay_single_until_churn",
        ) {
            return;
        }
        const CID: u32 = 0x0C3C_89A1;
        let terminal = shaped(pinned_shape(40_000));
        let mut receivers: Vec<_> = (0..WAYS + 2)
            .map(|i| {
                let mut r = shaped(receiver_shape(CID, 40_001 + i as u64));
                r.class_id = CID;
                r
            })
            .collect();
        let w = Walk {
            holder: &*terminal as *const ObjectHeader as usize,
            holder_shape: terminal.parent_class_id,
            slot: None,
            hops: NO_HOPS,
            depth: 1,
            getter: 0,
        };
        let mut cache = [0i64; crate::object::PIC_CACHE_WORDS];
        cache[HOLDER_STATE] = STATE_REGISTERED;
        for r in &mut receivers[..WAYS] {
            unsafe { publish(&mut cache, &**r, &w) };
        }
        let s = unsafe { site(&cache).unwrap() };
        assert_eq!(s.entries.iter().filter(|e| e.token != 0).count(), WAYS);
        assert!(s.entries.iter().all(|e| !e.multi_absent()));
        assert_eq!((s.next >> CURSOR_BITS) & SHARED_MASK, 0);
        unsafe { publish(&mut cache, &*receivers[WAYS], &w) };
        let s = unsafe { site(&cache).unwrap() };
        assert_eq!(s.entries.iter().filter(|e| e.token != 0).count(), WAYS);
        assert!(s.entries.iter().all(|e| !e.multi_absent()));
        assert_ne!(s.next & ABSENT_CHURN, 0);
        unsafe { publish(&mut cache, &*receivers[WAYS + 1], &w) };
        // The one displaced shape is learned on its next confirmed miss.
        unsafe { publish(&mut cache, &*receivers[0], &w) };
        let s = unsafe { site(&cache).unwrap() };
        assert_eq!(s.entries.iter().filter(|e| e.token != 0).count(), 1);
        assert_eq!(s.entries[0].slot & !MULTI_ABSENT, (WAYS + 2) as u32);
        assert_eq!((s.next >> CURSOR_BITS) & SHARED_MASK, 1);
        for r in &receivers {
            let token = (u64::from(r.parent_class_id) | PIC_ID_TOKEN_BIT) as i64;
            assert_eq!(
                unsafe { leaf_answer(&cache, &**r, token) },
                Some(crate::value::TAG_UNDEFINED)
            );
        }
        let record = (cache[SITE_WORD] as u64 & crate::value::POINTER_MASK) as usize as *mut Site;
        let data = Walk { slot: Some(0), ..w };
        // Point the cursor at the shared way: reclaimed empties must take
        // priority, even when ordinary rotation would evict the proof.
        unsafe { (*record).next &= !CURSOR_MASK };
        let mut fillers = Vec::new();
        for i in 1..WAYS {
            let cid = CID + i as u32;
            let mut r = shaped(receiver_shape(cid, 41_000 + i as u64));
            r.class_id = cid;
            unsafe { publish(&mut cache, &*r, &data) };
            fillers.push(r);
        }
        // Fill reclaimed ways with unrelated data proofs, then replace a
        // data way. Advancing the cursor must retain the shared-way bits.
        unsafe { (*record).next = ((*record).next & !CURSOR_MASK) | 1 };
        let cid = CID + WAYS as u32;
        let mut extra = shaped(receiver_shape(cid, 42_000));
        extra.class_id = cid;
        unsafe { publish(&mut cache, &*extra, &data) };
        assert_eq!((unsafe { (*record).next } >> CURSOR_BITS) & SHARED_MASK, 1);
        assert_ne!(unsafe { (*record).next } & ABSENT_CHURN, 0);
        for r in &receivers {
            let token = (u64::from(r.parent_class_id) | PIC_ID_TOKEN_BIT) as i64;
            assert_eq!(
                unsafe { leaf_answer(&cache, &**r, token) },
                Some(crate::value::TAG_UNDEFINED)
            );
        }
        unsafe { (*record).entries[0].drop_block() };
        unsafe { drop(Box::from_raw(record)) };
    }

    #[test]
    fn non_absent_and_same_receiver_refreshes_do_not_arm_sharing() {
        if !crate::object::method_site::run_with_fresh_worker_gate(
            "non_absent_and_same_receiver_refreshes_do_not_arm_sharing",
        ) {
            return;
        }
        const CID: u32 = 0x0C3C_89A2;
        let holder = shaped(pinned_shape(43_000));
        let absent = deep_walk(&[], &holder);
        let data = Walk {
            slot: Some(0),
            ..absent
        };
        let mut receivers: Vec<_> = (0..WAYS * 3)
            .map(|i| {
                let mut r = shaped(receiver_shape(CID, 43_001 + i as u64));
                r.class_id = CID;
                r
            })
            .collect();
        let mut cache = [0i64; crate::object::PIC_CACHE_WORDS];
        cache[HOLDER_STATE] = STATE_REGISTERED;
        for r in &receivers {
            unsafe { publish(&mut cache, &**r, &data) };
        }
        let record = (cache[SITE_WORD] as u64 & crate::value::POINTER_MASK) as usize as *mut Site;
        assert_eq!(unsafe { (*record).next } & ABSENT_CHURN, 0);
        assert!(unsafe { &(*record).entries }
            .iter()
            .all(|e| !e.multi_absent()));
        for e in unsafe { &(*record).entries } {
            unsafe { e.drop_block() };
        }
        unsafe { drop(Box::from_raw(record)) };
        cache = [0i64; crate::object::PIC_CACHE_WORDS];
        cache[HOLDER_STATE] = STATE_REGISTERED;
        for _ in 0..WAYS * 3 {
            unsafe { publish(&mut cache, &*receivers[0], &absent) };
        }
        let record = (cache[SITE_WORD] as u64 & crate::value::POINTER_MASK) as usize as *mut Site;
        assert_eq!(unsafe { (*record).next } & ABSENT_CHURN, 0);
        assert!(unsafe { &(*record).entries }
            .iter()
            .all(|e| !e.multi_absent()));
        for e in unsafe { &(*record).entries } {
            unsafe { e.drop_block() };
        }
        unsafe { drop(Box::from_raw(record)) };
        receivers.clear();
    }

    /// After observed churn, 96 receiver shapes coalesce into ONE way. Every guard
    /// remains necessary and only its chain prefix is enumerated as roots.
    #[test]
    fn multi_absent_shares_chain_and_checks_every_shape() {
        if !crate::object::method_site::run_with_fresh_worker_gate(
            "multi_absent_shares_chain_and_checks_every_shape",
        ) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        const CID: u32 = 0x0C3C_7A10;
        let base = crate::object::shapes::SHAPE_ID_BASE;
        let chain: Vec<_> = (0..8).map(|i| shaped(pinned_shape(10_000 + i))).collect();
        let holder = shaped(base + 200);
        let w = deep_walk(&chain, &holder);
        let mut receivers: Vec<_> = (1..=96)
            .map(|i| {
                let mut r = shaped(receiver_shape(CID, i));
                r.class_id = CID;
                r
            })
            .collect();
        let mut cache = [0i64; crate::object::PIC_CACHE_WORDS];
        cache[HOLDER_STATE] = STATE_REGISTERED;
        let before = stats().0;
        for r in receivers.iter().chain(&receivers) {
            unsafe { publish(&mut cache, &**r, &w) };
        }
        let record = (cache[SITE_WORD] as u64 & crate::value::POINTER_MASK) as usize as *mut Site;
        let s = unsafe { &mut *record };
        assert_eq!(s.entries.iter().filter(|e| e.token != 0).count(), 1);
        assert_eq!(stats().0 - before, (WAYS + 1) as u64);
        let e = &mut s.entries[0];
        assert!(e.multi_absent(), "the multi-shape proof must be exercised");
        assert_eq!(
            (unsafe { site(&cache).unwrap().next } >> CURSOR_BITS) & SHARED_MASK,
            1
        );
        assert_ne!(unsafe { site(&cache).unwrap().next } & ABSENT_CHURN, 0);
        assert_eq!(e.slot & !MULTI_ABSENT, 96);
        assert_eq!(unsafe { e.hops() }.len(), 8);
        assert_eq!(std::mem::size_of::<Entry>(), 48);
        // Fake hops are not registered shapes. Exercise the same pinned
        // shape comparisons the real admitted walk proves.
        for r in &receivers {
            let token = (PIC_ID_TOKEN_BIT | u64::from(r.parent_class_id)) as i64;
            assert_eq!(
                unsafe { answer(e, &**r) },
                Some(crate::value::TAG_UNDEFINED)
            );
            assert_eq!(
                unsafe { leaf_answer(&cache, &**r, token) },
                Some(crate::value::TAG_UNDEFINED),
            );
        }
        let r = &mut *receivers[47];
        let original = r.parent_class_id;
        r.parent_class_id = receiver_shape(CID, 1000);
        let token = (PIC_ID_TOKEN_BIT | u64::from(r.parent_class_id)) as i64;
        assert_eq!(
            unsafe { answer(e, r) },
            None,
            "an own add/getter/relink restamps"
        );
        assert_eq!(unsafe { leaf_answer(&cache, r, token) }, None);
        r.parent_class_id = original;
        let token = (PIC_ID_TOKEN_BIT | u64::from(original)) as i64;
        for (i, hop) in chain.iter().enumerate() {
            let ptr = &**hop as *const ObjectHeader as *mut ObjectHeader;
            let original = unsafe { (*ptr).parent_class_id };
            unsafe { (*ptr).parent_class_id = base + 500 };
            assert_eq!(unsafe { answer(e, r) }, None, "hop {i} changed");
            assert_eq!(
                unsafe { leaf_answer(&cache, r, token) },
                None,
                "leaf hop {i}"
            );
            unsafe { (*ptr).parent_class_id = original };
        }
        let ptr = &*holder as *const ObjectHeader as *mut ObjectHeader;
        unsafe { (*ptr).parent_class_id = base + 501 };
        assert_eq!(unsafe { answer(e, r) }, None, "terminal changed");
        assert_eq!(unsafe { leaf_answer(&cache, r, token) }, None);
        unsafe { (*ptr).parent_class_id = base + 200 };
        let mut seen = Vec::new();
        let mut mark = |v: f64| seen.push(v.to_bits() & crate::value::POINTER_MASK);
        scan_roots(
            &mut cache,
            &mut crate::gc::RuntimeRootVisitor::for_copy(&mut mark),
        );
        assert_eq!(
            seen.len(),
            9,
            "receiver ids must not be visited as pointers"
        );
        for hop in &chain {
            assert!(seen.contains(&((&**hop as *const ObjectHeader) as u64)));
        }
        assert!(seen.contains(&((&*holder as *const ObjectHeader) as u64)));
        // A new chain for a member replaces the owned group allocation.
        let short = deep_walk(&chain[..1], &holder);
        unsafe { publish(&mut cache, r, &short) };
        assert_eq!(s.entries[0].depth, 2);
        assert!(!s.entries[0].multi_absent());
        assert_eq!((s.next >> CURSOR_BITS) & SHARED_MASK, 0);
        unsafe { s.entries[0].drop_block() };
        unsafe { drop(Box::from_raw(record)) };
    }

    #[test]
    fn multi_absent_bound_and_prototype_identity_are_enforced() {
        if !crate::object::method_site::run_with_fresh_worker_gate(
            "multi_absent_bound_and_prototype_identity_are_enforced",
        ) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        const CID: u32 = 0x0C3C_7A11;
        let base = crate::object::shapes::SHAPE_ID_BASE;
        let holder = shaped(base + 200);
        let w = deep_walk(&[], &holder);
        let mut cache = [0i64; crate::object::PIC_CACHE_WORDS];
        cache[HOLDER_STATE] = STATE_REGISTERED;
        let receivers: Vec<_> = (1..=ABSENT_RECEIVERS + 5)
            .map(|i| {
                let mut r = shaped(receiver_shape(CID, u64::from(i)));
                r.class_id = CID;
                r
            })
            .collect();
        for r in receivers.iter().chain(&receivers) {
            unsafe { publish(&mut cache, &**r, &w) };
        }
        let record = (cache[SITE_WORD] as u64 & crate::value::POINTER_MASK) as usize as *mut Site;
        let s = unsafe { &mut *record };
        assert_eq!(s.entries.iter().filter(|e| e.token != 0).count(), 2);
        assert_eq!(s.entries[0].slot & !MULTI_ABSENT, ABSENT_RECEIVERS);
        // The class id and terminal alone do not prove a common direct link.
        let shape = crate::object::shapes::shape_descriptor_ensure_with_generation(
            std::ptr::null(),
            0,
            0,
            2000,
            crate::object::shapes::ShapeObjectKind::Ordinary,
            PROTO_ID_MIXED | u64::from(CID),
            crate::object::shapes::ReceiverFacts::NONE,
        )
        .unwrap();
        let mut other = shaped(shape);
        other.class_id = CID;
        unsafe { publish(&mut cache, &*other, &w) };
        assert_eq!(s.entries.iter().filter(|e| e.token != 0).count(), 3);
        for r in &receivers {
            let token = (PIC_ID_TOKEN_BIT | u64::from(r.parent_class_id)) as i64;
            assert!(s
                .entries
                .iter()
                .any(|e| unsafe { e.matches_receiver(token, CID) }));
        }
        for e in &s.entries {
            unsafe { e.drop_block() };
        }
        unsafe { drop(Box::from_raw(record)) };
    }

    #[test]
    fn multi_absent_reproves_shared_class_link_after_generation_change() {
        if !crate::object::method_site::run_with_fresh_worker_gate(
            "multi_absent_reproves_shared_class_link_after_generation_change",
        ) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        // Real GC headers are required by stated_link; keep this fixture
        // in a no-move scope while its raw receiver vector is constructed.
        let _no_move = crate::gc::GcSuppressScope::new();
        const CID: u32 = 0x0C3C_7A12;
        const PROTO_CID: u32 = 0x0C3C_7A13;
        let keys = crate::object::js_build_class_keys_array(PROTO_CID, 1, b"marker".as_ptr(), 6, 0);
        let shape = crate::object::shapes::js_object_shape_id_for_class_keys(
            keys as usize as u64,
            1,
            PROTO_CID,
            0,
        );
        let a = crate::object::js_object_alloc_class_inline_keys_stamped(
            PROTO_CID, 0, 1, keys, shape, 0,
        );
        crate::object::class_decl_prototype_object_root_store(CID, a);
        let b = crate::object::js_object_alloc_class_inline_keys_stamped(
            PROTO_CID, 0, 1, keys, shape, 0,
        );
        let a = crate::object::class_decl_prototype_object(CID);
        assert_ne!(a, b);
        assert_eq!(unsafe { object_shape_stamp(a) }, unsafe {
            object_shape_stamp(b)
        });
        let receivers: Vec<_> = (1..=24)
            .map(|i| {
                let name = (0..i)
                    .map(|n| format!("own{n}"))
                    .collect::<Vec<_>>()
                    .join("\0");
                let keys = crate::object::js_build_class_keys_array(
                    CID,
                    i,
                    name.as_ptr(),
                    name.len() as u32,
                    0,
                );
                let shape = crate::object::shapes::js_object_shape_id_for_class_keys(
                    keys as usize as u64,
                    i,
                    CID,
                    0,
                );
                crate::object::js_object_alloc_class_inline_keys_stamped(CID, 0, i, keys, shape, 0)
            })
            .collect();
        let w = Walk {
            holder: a as usize,
            holder_shape: unsafe { object_shape_stamp(a) },
            slot: None,
            hops: NO_HOPS,
            depth: 1,
            getter: 0,
        };
        let mut cache = [0i64; crate::object::PIC_CACHE_WORDS];
        cache[HOLDER_STATE] = STATE_REGISTERED;
        for r in receivers.iter().chain(&receivers) {
            assert_eq!(unsafe { class_link(*r) }, Some(a as *const ObjectHeader));
            unsafe { publish(&mut cache, *r, &w) };
        }
        let record = (cache[SITE_WORD] as u64 & crate::value::POINTER_MASK) as usize as *mut Site;
        let e = unsafe { &(*record).entries[0] };
        assert!(e.multi_absent());
        assert_eq!(e.slot & !MULTI_ABSENT, 24);
        crate::object::class_registry::class_lookup_surface_gen_bump();
        for r in &receivers {
            let token = (PIC_ID_TOKEN_BIT | u64::from(unsafe { object_shape_stamp(*r) })) as i64;
            assert_eq!(unsafe { leaf_answer(&cache, *r, token) }, None);
        }
        assert_eq!(
            unsafe { try_shared_hit(record, receivers[5]) },
            Some(crate::value::TAG_UNDEFINED)
        );
        // A same-identity generation re-proof covers the whole set.
        for r in &receivers {
            let token = (PIC_ID_TOKEN_BIT | u64::from(unsafe { object_shape_stamp(*r) })) as i64;
            assert_eq!(
                unsafe { leaf_answer(&cache, *r, token) },
                Some(crate::value::TAG_UNDEFINED)
            );
        }
        crate::object::class_decl_prototype_object_root_store(CID, b);
        for r in &receivers {
            let token = (PIC_ID_TOKEN_BIT | u64::from(unsafe { object_shape_stamp(*r) })) as i64;
            assert_eq!(unsafe { leaf_answer(&cache, *r, token) }, None);
            assert_eq!(unsafe { try_shared_hit(record, *r) }, None);
        }
        unsafe { (*record).entries[0].drop_block() };
        unsafe { drop(Box::from_raw(record)) };
    }

    #[test]
    fn collecting_shared_reads_check_receiver_and_chain() {
        if !crate::object::method_site::run_with_fresh_worker_gate(
            "collecting_shared_reads_check_receiver_and_chain",
        ) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        const CID: u32 = 0x0C3C_7A14;
        let mut chain = vec![shaped(pinned_shape(11_000))];
        let mut holder = shaped(crate::object::shapes::SHAPE_ID_BASE + 300);
        let w = deep_walk(&chain, &holder);
        let mut receivers: Vec<_> = (1..=24)
            .map(|i| {
                let mut r = shaped(receiver_shape(CID, i));
                r.class_id = CID;
                r
            })
            .collect();
        let mut cache = [0i64; crate::object::PIC_CACHE_WORDS];
        cache[HOLDER_STATE] = STATE_REGISTERED;
        for r in receivers.iter().chain(&receivers) {
            unsafe { publish(&mut cache, &**r, &w) };
        }
        let record = (cache[SITE_WORD] as u64 & crate::value::POINTER_MASK) as usize as *mut Site;
        let r = &mut *receivers[5];
        assert_eq!(
            unsafe { try_shared_hit(record, r) },
            Some(crate::value::TAG_UNDEFINED)
        );
        let shape = r.parent_class_id;
        r.parent_class_id = receiver_shape(CID, 1000);
        assert_eq!(
            unsafe { try_shared_hit(record, r) },
            None,
            "receiver restamped"
        );
        r.parent_class_id = shape;
        r.class_id = CID + 1;
        assert_eq!(
            unsafe { try_shared_hit(record, r) },
            None,
            "different class"
        );
        r.class_id = CID;
        let shape = chain[0].parent_class_id;
        chain[0].parent_class_id = crate::object::shapes::SHAPE_ID_BASE + 301;
        assert_eq!(unsafe { try_shared_hit(record, r) }, None, "hop restamped");
        chain[0].parent_class_id = shape;
        let shape = holder.parent_class_id;
        holder.parent_class_id = crate::object::shapes::SHAPE_ID_BASE + 302;
        assert_eq!(
            unsafe { try_shared_hit(record, r) },
            None,
            "terminal restamped"
        );
        holder.parent_class_id = shape;
        assert_eq!(
            unsafe { try_shared_hit(record, r) },
            Some(crate::value::TAG_UNDEFINED)
        );
        for e in unsafe { &(*record).entries } {
            unsafe { e.drop_block() };
        }
        unsafe { drop(Box::from_raw(record)) };
    }

    #[test]
    fn unpinned_hop_is_not_published() {
        if !crate::object::method_site::run_with_fresh_worker_gate("unpinned_hop_is_not_published")
        {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        const CID: u32 = 0x0C3C_7A14;
        let recv = shaped(receiver_shape(CID, 1));
        let chain = vec![shaped(receiver_shape(CID, 2))];
        let holder = shaped(pinned_shape(30_000));
        let w = deep_walk(&chain, &holder);
        let mut cache = [0i64; crate::object::PIC_CACHE_WORDS];
        cache[HOLDER_STATE] = STATE_REGISTERED;
        unsafe { publish(&mut cache, &*recv, &w) };
        assert!(
            unsafe { site(&cache) }.is_none(),
            "a bare CLASS hop cannot pin the next prototype by shape"
        );
    }

    /// Fake objects whose only meaningful word is their ShapeId.
    pub(super) fn shaped(shape: u32) -> Box<ObjectHeader> {
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
            .map(|i| shaped(pinned_shape(20_000 + u64::from(i))))
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
        // Fake headers carry admitted shape descriptors; the comparison
        // checks below exercise every shape the real walk records.
        let mut e = *e;
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
            let original = unsafe { (*hop).parent_class_id };
            unsafe { (*hop).parent_class_id = base + 300 };
            assert_eq!(unsafe { pinned_answer(&e) }, None, "hop {i} moved");
            unsafe { (*hop).parent_class_id = original };
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
            pinned_hops: true,
            slot: 0,
            holder: a as usize,
            holder_shape: proto_shape,
            hops: std::ptr::null_mut(),
            generation: 0,
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

#[cfg(test)]
#[path = "class_read/retirement_tests.rs"]
mod retirement_tests;

/// Inspect the real site in the moving-GC witness; no production API.
#[cfg(test)]
pub(crate) unsafe fn test_retirement_snapshot(c: &PicCache) -> (usize, bool, u32) {
    let s = site(c).expect("class site was primed");
    (
        s.entries.iter().filter(|e| e.token != 0).count(),
        s.next & ABSENT_CHURN != 0,
        s.entries
            .iter()
            .filter(|e| e.multi_absent())
            .map(|e| e.slot & !MULTI_ABSENT)
            .sum(),
    )
}

#[cfg(test)]
pub(crate) unsafe fn test_shared_primary(c: &PicCache) -> u32 {
    site(c)
        .unwrap()
        .entries
        .iter()
        .find(|e| e.multi_absent())
        .expect("shared way")
        .token as u32
}
