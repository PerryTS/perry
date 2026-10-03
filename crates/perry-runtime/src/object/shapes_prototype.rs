//! The [[Prototype]] as a fact of the shape.
//!
//! A ShapeId's `proto_id` already names its receivers' prototype: two objects
//! share a ShapeId only if they share a prototype (`shapes::object_proto_id`).
//! This module adds the way back from that identity to the object, so a
//! receiver needs no per-instance record of its prototype: its ShapeId names
//! the identity, and the identity's WORD is the receiver's [[Prototype]].
//!
//! * Which identities have a word: [`proto_id_carries_word`]. These are a
//!   recorded prototype's serial, `MIXED` (class | serial, which uses the
//!   serial's word) and `UNIQUE` (a prototype with no serial, one identity per
//!   link). Null, the default, class-implied and per-object identities answer
//!   by themselves (`shapes::object_prototype_word`).
//! * One word per IDENTITY, not per shape record: every shape of one
//!   prototype names the same word, and the record stays one cache line. The
//!   words live in stable pages (an address never moves). The pages are
//!   published to a thread-local directory, so a read is the record's
//!   `proto_id` plus two loads. The prototype funnel
//!   (`shapes::transition_object_shape_prototype`) writes the word before any
//!   shape names the identity.
//! * Who reads it: a function constructor's instance, the receiver the funnel
//!   links without a meta record. A receiver that has a meta record anyway
//!   (link flags, a per-object identity) keeps the same bits in it, which is
//!   the cheaper read.
//! * GC: the word is a traced edge of its CARRIERS, like the record's keys
//!   word (`gc_shape_prototype_edge_slot`), so a prototype lives exactly while
//!   something reaches it.
//!   - In a full trace only the first carrier of each identity emits the edge
//!     ([`identity_edge_slot`]): that visit marks the prototype.
//!   - A minor never traces an old carrier, so [`scan_shape_prototype_words_mut`]
//!     roots every word that names a young object (the young log).
//!   - Rewrite passes repair every word through forwarding.
//!   - [`prune_dead_shape_prototypes`] clears a word whose prototype died.
//! * Workers: the seed copies no record whose identity has a word, so a
//!   pointer never crosses agents.

use std::cell::Cell;

use super::{PROTO_ID_CLASS, PROTO_ID_DEFAULT, PROTO_ID_MIXED, PROTO_ID_NULL, PROTO_ID_PER_OBJECT};

/// The tag bits of a prototype identity (`shapes::PROTO_ID_*`).
const PROTO_ID_TAG_MASK: u64 = 3 << 62;
/// The serial bits of a `MIXED` identity (`shapes::PROTO_ID_MIXED_SERIAL_BITS`).
const MIXED_SERIAL_MASK: u64 = (1 << 30) - 1;
const PAGE_SHIFT: usize = 10;
const PAGE_LEN: usize = 1 << PAGE_SHIFT;
const PAGE_MASK: usize = PAGE_LEN - 1;

/// One page of identity words, with the full-trace epoch each word's edge
/// was last emitted in (`identity_edge_slot`).
struct PageData {
    words: [u64; PAGE_LEN],
    emitted: [u32; PAGE_LEN],
}

type Page = Box<PageData>;

/// Does identity `proto_id` name its prototype through a word? False for an
/// identity that answers by itself: the realm's default, a compiled class's,
/// a per-object one, and null.
#[inline]
pub(crate) fn proto_id_carries_word(proto_id: u64) -> bool {
    proto_id != PROTO_ID_DEFAULT
        && proto_id != PROTO_ID_NULL
        && proto_id != PROTO_ID_PER_OBJECT
        && proto_id & PROTO_ID_TAG_MASK != PROTO_ID_CLASS
}

/// The (band, index) of identity `proto_id`'s word: band 0 is indexed by
/// prototype serial (a plain serial identity and a `MIXED` one share it — one
/// object), band 1 by `UNIQUE` number.
#[inline]
fn word_key(proto_id: u64) -> Option<(usize, usize)> {
    if !proto_id_carries_word(proto_id) {
        return None;
    }
    Some(match proto_id & PROTO_ID_TAG_MASK {
        0 => (0, proto_id as usize),
        PROTO_ID_MIXED => (0, (proto_id & MIXED_SERIAL_MASK) as usize),
        _ => (1, (proto_id & !PROTO_ID_TAG_MASK) as usize),
    })
}

/// The agent's published page directory, one per band: read with no
/// `state()` fetch, as `shapes_store::AGENT_SHAPE_DIR` is.
struct WordDir {
    pages: Cell<*mut Option<Page>>,
    len: Cell<usize>,
}

impl WordDir {
    const fn empty() -> Self {
        WordDir {
            pages: Cell::new(std::ptr::null_mut()),
            len: Cell::new(0),
        }
    }
}

#[thread_local]
static AGENT_WORD_DIR: [WordDir; 2] = [WordDir::empty(), WordDir::empty()];

/// The current full trace, counted from 1 (`note_full_trace_begin`). A word
/// whose edge was emitted in this trace is not emitted again: the first
/// carrier marked the prototype, and every later visit would only re-test it.
#[thread_local]
static FULL_TRACE_EPOCH: Cell<u32> = Cell::new(0);

/// A full trace begins (`gc::full_trace::begin_full_trace`).
pub(crate) fn note_full_trace_begin() {
    FULL_TRACE_EPOCH.set(FULL_TRACE_EPOCH.get().wrapping_add(1).max(1));
}

/// Identity -> [[Prototype]] words, per agent (owned by the agent's shape
/// slab). Pages are allocated on first use and never move or shrink.
#[derive(Default)]
pub(crate) struct ProtoWords {
    bands: [Vec<Option<Page>>; 2],
    /// Word addresses that named a nursery object when written or last
    /// visited: the minor's complete candidate set.
    young: Vec<*mut u64>,
    agent: bool,
}

#[inline]
fn bits_in_nursery(bits: u64) -> bool {
    let value = crate::value::JSValue::from_bits(bits);
    value.is_pointer() && crate::arena::pointer_in_nursery(value.as_pointer::<u8>() as usize)
}

impl ProtoWords {
    /// This is the agent slab's: publish its directory.
    pub(super) fn make_agent(&mut self) {
        self.agent = true;
        self.publish();
    }

    fn publish(&mut self) {
        if !self.agent {
            return;
        }
        for (band, dir) in AGENT_WORD_DIR.iter().enumerate() {
            dir.pages.set(self.bands[band].as_mut_ptr());
            dir.len.set(self.bands[band].len());
        }
    }

    pub(super) fn unpublish(&self) {
        if !self.agent {
            return;
        }
        for dir in &AGENT_WORD_DIR {
            dir.pages.set(std::ptr::null_mut());
            dir.len.set(0);
        }
    }

    /// Drop every word (a test resetting the shape table).
    #[cfg(test)]
    pub(super) fn reset(&mut self) {
        let agent = self.agent;
        *self = ProtoWords::default();
        self.agent = agent;
        self.publish();
    }

    fn slot_ensure(&mut self, band: usize, index: usize) -> *mut u64 {
        let page = index >> PAGE_SHIFT;
        let pages = &mut self.bands[band];
        if page >= pages.len() {
            pages.resize_with(page + 1, || None);
            self.publish();
        }
        let page = self.bands[band][page].get_or_insert_with(|| {
            Box::new(PageData {
                words: [0; PAGE_LEN],
                emitted: [0; PAGE_LEN],
            })
        });
        &mut page.words[index & PAGE_MASK]
    }

    /// Visit every word that holds a heap reference.
    fn for_each_word(&mut self, mut f: impl FnMut(*mut u64)) {
        for pages in &mut self.bands {
            for page in pages.iter_mut().flatten() {
                for word in page.words.iter_mut() {
                    if crate::value::JSValue::from_bits(*word).is_pointer() {
                        f(word);
                    }
                }
            }
        }
    }
}

/// The address of identity `proto_id`'s word in this agent, if it was ever
/// written. Two loads; no `state()` fetch.
#[inline]
pub(crate) fn identity_word_slot(proto_id: u64) -> Option<*mut u64> {
    let (band, index) = word_key(proto_id)?;
    let dir = &AGENT_WORD_DIR[band];
    let page = index >> PAGE_SHIFT;
    if page >= dir.len.get() {
        return None;
    }
    // SAFETY: the published directory is the agent's own page vector, current
    // as of its last growth; a present page is a stable boxed array.
    unsafe {
        (*dir.pages.get().add(page))
            .as_ref()
            .map(|page| &page.words[index & PAGE_MASK] as *const u64 as *mut u64)
    }
}

/// The GC edge of identity `proto_id`'s word for a carrier being traced: the
/// word's address when it holds a heap reference. In a FULL trace (`dedupe`)
/// only the first carrier emits it — that visit marks the prototype, and the
/// trace's rewrite needs no carrier at all (`scan_shape_prototype_words_mut`
/// repairs every word) — so the thousands of instances of one constructor
/// do not each re-test one marked object. A minor emits it for every carrier.
#[inline]
pub(crate) fn identity_edge_slot(proto_id: u64, dedupe: bool) -> Option<*mut u64> {
    let (band, index) = word_key(proto_id)?;
    let dir = &AGENT_WORD_DIR[band];
    let page = index >> PAGE_SHIFT;
    if page >= dir.len.get() {
        return None;
    }
    // SAFETY: the published directory is the agent's own page vector; a
    // present page is a stable boxed page, and the collector owns the agent.
    unsafe {
        let page: *mut PageData = &mut **(*dir.pages.get().add(page)).as_mut()?;
        let index = index & PAGE_MASK;
        let word = &mut (*page).words[index];
        if !crate::value::JSValue::from_bits(*word).is_pointer() {
            return None;
        }
        if dedupe {
            let epoch = FULL_TRACE_EPOCH.get();
            if (*page).emitted[index] == epoch {
                return None;
            }
            (*page).emitted[index] = epoch;
        }
        Some(word)
    }
}

/// The [[Prototype]] bits identity `proto_id` names, or 0.
#[inline]
pub(crate) fn identity_prototype_word(proto_id: u64) -> u64 {
    // SAFETY: a published word slot of this agent.
    identity_word_slot(proto_id).map_or(0, |slot| unsafe { *slot })
}

/// The prototype word of shape `id`: the receiver's [[Prototype]] bits when
/// the shape's identity names an object, else 0.
#[inline]
pub(crate) fn shape_prototype_word(id: u32) -> u64 {
    // SAFETY: `agent_record` never returns null; an absent id reads the
    // shared empty record, whose identity is the default.
    identity_prototype_word(unsafe { (*super::shapes_store::ShapeSlab::agent_record(id)).proto_id })
}

/// Make identity `proto_id` name `bits`, before any shape names it (the
/// prototype funnel). An identity names one object for its whole life, so
/// the write only ever fills an empty word or repeats the same bits — or
/// replaces bits whose object died (the prune clears those).
pub(super) fn write_identity_word(proto_id: u64, bits: u64) {
    let Some((band, index)) = word_key(proto_id) else {
        return;
    };
    let table = &crate::state::state().shapes;
    // SAFETY: one agent, one thread; no slab reference is held across this.
    let words = unsafe { &mut table.slab_mut().protos };
    let slot = words.slot_ensure(band, index);
    // SAFETY: a stable word of this agent's pages.
    unsafe {
        // GC_STORE_AUDIT(ROOT): identity word, scanned by
        // scan_shape_prototype_words_mut and traced through its carriers.
        *slot = bits;
    }
    if bits_in_nursery(bits) {
        words.young.push(slot);
    }
}

/// The identity words outside the per-carrier edge.
///
/// Marking: a receiver the collector traces emits its identity's word itself
/// (`gc_shape_prototype_edge_slot`), so a full mark roots nothing here and a
/// prototype whose last carrier died is collectable. A minor never traces an
/// OLD carrier, so it roots every word naming a young object — the young log
/// is the complete set.
///
/// Rewriting: every word is repaired through forwarding.
pub(crate) fn scan_shape_prototype_words_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let table = &crate::state::state().shapes;
    // SAFETY: the collector runs on the owning agent with the mutator
    // stopped; no other slab reference is live.
    let words = unsafe { &mut table.slab_mut().protos };
    if visitor.young_scope() {
        let mut log = std::mem::take(&mut words.young);
        log.sort_unstable();
        log.dedup();
        let mut young = Vec::with_capacity(log.len());
        for slot in log {
            // SAFETY: a stable word of this agent's pages.
            unsafe {
                if !bits_in_nursery(*slot) {
                    continue;
                }
                visitor.visit_nanbox_u64_slot(&mut *slot);
                if bits_in_nursery(*slot) {
                    young.push(slot);
                }
            }
        }
        words.young = young;
    } else if visitor.is_metadata_rewrite_phase() {
        let mut young = Vec::new();
        words.for_each_word(|slot| {
            // SAFETY: a stable word of this agent's pages.
            unsafe {
                visitor.visit_nanbox_u64_slot(&mut *slot);
                if bits_in_nursery(*slot) {
                    young.push(slot);
                }
            }
        });
        words.young = young;
    }
}

/// Post-trace prune: clear a word whose prototype died. No live receiver can
/// carry its identity (a carrier traces the word), so nothing reads it again.
pub(crate) fn prune_dead_shape_prototypes(is_dead_owner: &dyn Fn(usize) -> bool) {
    let table = &crate::state::state().shapes;
    // SAFETY: one agent, one thread; no slab reference is held across this.
    let words = unsafe { &mut table.slab_mut().protos };
    words.for_each_word(|slot| {
        // SAFETY: a stable word of this agent's pages.
        unsafe {
            let value = crate::value::JSValue::from_bits(*slot);
            if is_dead_owner(value.as_pointer::<u8>() as usize) {
                *slot = 0;
            }
        }
    });
    words.young.retain(|&slot| unsafe { *slot } != 0);
}

#[cfg(test)]
#[path = "shapes_prototype_tests.rs"]
mod tests;
