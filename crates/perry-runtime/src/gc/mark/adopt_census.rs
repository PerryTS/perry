//! Census facts of promoted blocks, recorded by the in-place promotion walk and
//! adopted by the promoted-cohort full that follows it (#10182).
//!
//! # Why
//!
//! A promoted-cohort full (`gc::promoted_cohort`) runs at the same precise
//! safepoint as the nursery minor whose promotion made it due. That minor's
//! in-place promotion has just parsed every header of every block it
//! promoted (`arena::promote::stamp_and_index_block`), and the full's census
//! would parse them all again before the mutator has run a single instruction.
//! On `records_array_20m:parse` those blocks are two of the three JSON trees
//! the census reads. So the promotion walk records, per block, exactly what the
//! census records — the start bitmap and the per-block facts — and the census
//! adopts the record instead of walking the block.
//!
//! # Why the record equals the walk
//!
//! * **Same headers.** The promotion walk and the census walk use the same
//!   alignment and hop; a record is kept only when the promotion walk parsed its
//!   block to the bump offset without stopping on an implausible size, and a
//!   parse that did not stop there cannot stop in the census walk either (the
//!   census's size guard is weaker).
//! * **Same facts, read after the promotion's own header writes.** Each header
//!   is noted after `stamp_header_promoted_in_place`, and the flag facts are
//!   computed by the census's own function (`census_header_flag_facts`).
//! * **Nothing writes those headers in between.** Recording is armed only for
//!   the nursery minor of one safepoint; the record can be adopted only by the
//!   cohort full started at that same safepoint, before any mutator code runs,
//!   and it is discarded when the safepoint returns. The rest of the minor
//!   clears trace marks. At adoption, traced records refresh header flag facts
//!   at their recorded starts, so stale marks do not disable block reclamation.
//!   Pinned and other exceptional-header obligations are re-read, not assumed.
//! * **Same block.** A record is adopted only for a block whose data address,
//!   bump offset and size are the ones the census snapshotted, and only for an
//!   old-generation block.
//!
//! Test builds re-walk every adopted block with the census walk and assert the
//! two agree, so every test that reaches adoption checks the argument.

use super::*;
use std::cell::RefCell;

/// One promoted block's census record.
pub(crate) struct AdoptableBlock {
    // Existing promotion mode is provenance of this census. Traced records
    // are adoptable, but must not enter the untraced-only controller probe.
    liveness: crate::arena::PromotionLiveness,
    extent: usize,
    size: usize,
    words: usize,
    chunks: Vec<Vec<u64>>,
    objects: usize,
    bytes: u64,
    first_start: usize,
    last_start: usize,
    /// Bit `t` of word `t >> 6` is set when an object of `obj_type == t` was
    /// recorded; the census applies its own per-type obligations at adoption.
    types: [u64; 4],
    flag_obligation: bool,
    premarked: bool,
    non_walkable: bool,
}

impl AdoptableBlock {
    /// The traced minor clears its mark list after recording. Re-read only
    /// header-dependent facts at the recorded starts before adoption, rather
    /// than carrying the old marks into the full sweep as exceptional objects.
    /// Pinned, forwarded, non-arena and raw-array obligations remain actual
    /// header facts; a completed trace is not permission to assume them away.
    ///
    /// # Safety
    /// The recorded starts still name readable headers of this same block.
    unsafe fn refresh_traced_header_facts(&mut self, data: usize) {
        if !matches!(self.liveness, crate::arena::PromotionLiveness::Marked) || !self.premarked {
            return;
        }
        let mut obligation = false;
        let mut premarked = false;
        'chunks: for (chunk_index, chunk) in self.chunks.iter().enumerate() {
            for (in_chunk, &word) in chunk.iter().enumerate() {
                let mut starts = word;
                while starts != 0 {
                    let bit = starts.trailing_zeros() as usize;
                    starts &= starts - 1;
                    let word_index = chunk_index * CENSUS_BITMAP_CHUNK_WORDS + in_chunk;
                    let offset = (word_index * 64 + bit) << CENSUS_START_ALIGN_SHIFT;
                    let header = (data + offset) as *const GcHeader;
                    let facts = block_skip::census_header_flag_facts(header);
                    obligation |= facts.0;
                    premarked |= facts.1;
                    if obligation && premarked {
                        break 'chunks;
                    }
                }
            }
        }
        self.flag_obligation = obligation;
        self.premarked = premarked;
    }
}

enum State {
    Off,
    Recording(crate::fast_hash::PtrHashMap<usize, AdoptableBlock>),
    Ready(crate::fast_hash::PtrHashMap<usize, AdoptableBlock>),
    Adopting(crate::fast_hash::PtrHashMap<usize, AdoptableBlock>),
}

crate::perry_thread_local! {
    static STATE: RefCell<State> = const { RefCell::new(State::Off) };
}

/// Record the census facts of the blocks the next promotion walk promotes.
pub(crate) fn begin_recording() {
    STATE.with(|s| *s.borrow_mut() = State::Recording(crate::fast_hash::new_ptr_hash_map()));
    // #10241: the minor about to run notes its own remembered set; nothing an
    // earlier safepoint noted may reach this one's cohort full.
    super::super::promoted_cohort::survival::clear_minor_remembered_parents();
}

pub(crate) fn recording() -> bool {
    STATE.with(|s| matches!(*s.borrow(), State::Recording(_)))
}

/// The minor is over: keep what it recorded for a full at this safepoint.
pub(crate) fn finish_recording() {
    let ready = STATE.with(|s| {
        let mut state = s.borrow_mut();
        *state = match std::mem::replace(&mut *state, State::Off) {
            State::Recording(map) if !map.is_empty() => State::Ready(map),
            _ => State::Off,
        };
        matches!(*state, State::Ready(_))
    });
    if !ready {
        super::super::promoted_cohort::survival::clear_minor_remembered_parents();
    }
}

/// The full about to start may adopt the records.
pub(crate) fn begin_adopting() {
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        *state = match std::mem::replace(&mut *state, State::Off) {
            State::Ready(map) => State::Adopting(map),
            _ => State::Off,
        };
    });
}

/// Drop every record: the safepoint is returning to the mutator.
pub(crate) fn discard() {
    let was_off = STATE.with(|s| {
        matches!(
            std::mem::replace(&mut *s.borrow_mut(), State::Off),
            State::Off
        )
    });
    // Every nursery safepoint discards; only one that recorded can have noted
    // a remembered set, so the common path leaves that thread-local untouched.
    if !was_off {
        super::super::promoted_cohort::survival::clear_minor_remembered_parents();
    }
}

/// `(data, extent, bytes)` for the existing untraced-only survival probe.
/// Traced census facts may be adopted, but must never enable new controller
/// feedback. Read before `begin_adopting` consumes the census records.
pub(crate) fn ready_blocks() -> Vec<(usize, usize, u64)> {
    STATE.with(|s| match &*s.borrow() {
        State::Ready(map)
            if map.values().all(|block| {
                matches!(
                    block.liveness,
                    crate::arena::PromotionLiveness::AssumeAllLive
                )
            }) =>
        {
            map.iter()
                .map(|(&data, block)| (data, block.extent, block.bytes))
                .collect()
        }
        _ => Vec::new(),
    })
}

/// Take the record of the block at `data`, if the census may adopt one.
fn take(data: usize) -> Option<AdoptableBlock> {
    STATE.with(|s| match &mut *s.borrow_mut() {
        State::Adopting(map) => map.remove(&data),
        _ => None,
    })
}

pub(crate) fn adopted_blocks() -> u64 {
    GC_STATS.with(|stats| stats.borrow().adopted_blocks)
}

/// Builds one block's record during the promotion walk. Inert when recording
/// is not armed or the block is too large for a start bitmap.
pub(crate) struct AdoptableBlockBuilder {
    data: usize,
    block: Option<AdoptableBlock>,
}

impl AdoptableBlockBuilder {
    pub(crate) fn begin(
        data: usize,
        offset: usize,
        size: usize,
        liveness: crate::arena::PromotionLiveness,
    ) -> Self {
        if offset == 0 || offset > CENSUS_BITMAP_MAX_EXTENT || !recording() {
            return Self { data, block: None };
        }
        let words = offset.div_ceil(1 << CENSUS_START_ALIGN_SHIFT).div_ceil(64);
        let mut chunks = Vec::with_capacity(CENSUS_BITMAP_MAX_CHUNKS);
        let mut start = 0;
        while start < words {
            chunks.push(vec![0u64; (words - start).min(CENSUS_BITMAP_CHUNK_WORDS)]);
            start += CENSUS_BITMAP_CHUNK_WORDS;
        }
        Self {
            data,
            block: Some(AdoptableBlock {
                liveness,
                extent: offset,
                size,
                words,
                chunks,
                objects: 0,
                bytes: 0,
                first_start: 0,
                last_start: 0,
                types: [0; 4],
                flag_obligation: false,
                premarked: false,
                non_walkable: false,
            }),
        }
    }

    /// Note the header at `aligned` bytes into the block, as the census would
    /// see it now.
    ///
    /// # Safety
    /// `header` is the header at `data + aligned`, below the block's offset.
    #[inline]
    pub(crate) unsafe fn note(&mut self, header: *const GcHeader, aligned: usize) {
        let Some(block) = self.block.as_mut() else {
            return;
        };
        let obj_type = (*header).obj_type;
        if !gc_type_is_arena_walkable(obj_type) {
            block.non_walkable = true;
            return;
        }
        let (flag_obligation, premarked) = block_skip::census_header_flag_facts(header);
        block.flag_obligation |= flag_obligation;
        block.premarked |= premarked;
        block.types[(obj_type >> 6) as usize] |= 1u64 << (obj_type & 63);
        let user_ptr = self.data + aligned + GC_HEADER_SIZE;
        if block.objects == 0 {
            block.first_start = user_ptr;
        }
        block.last_start = user_ptr;
        block.objects += 1;
        block.bytes += (*header).size as u64;
        let bit = aligned >> CENSUS_START_ALIGN_SHIFT;
        let word = bit >> 6;
        block.chunks[word >> CENSUS_BITMAP_CHUNK_WORD_SHIFT]
            [word & (CENSUS_BITMAP_CHUNK_WORDS - 1)] |= 1u64 << (bit & 63);
    }

    /// Keep the record when the walk parsed the whole block.
    pub(crate) fn finish(self, parsed_whole_block: bool) {
        let Some(mut block) = self.block else {
            return;
        };
        if !parsed_whole_block {
            return;
        }
        #[cfg(test)]
        if sabotage::dropping_starts() {
            for chunk in &mut block.chunks {
                chunk.fill(0);
            }
        }
        #[cfg(not(test))]
        let _ = &mut block;
        let data = self.data;
        STATE.with(|s| {
            if let State::Recording(map) = &mut *s.borrow_mut() {
                map.insert(data, block);
            }
        });
    }
}

impl ValidPointerSetBuilder {
    /// Adopt the promotion walk's record of this block instead of walking it,
    /// when one exists for exactly this block. Returns whether it did.
    ///
    /// # Safety
    /// As `census_whole_block`.
    pub(super) unsafe fn adopt_promoted_block(
        &mut self,
        block_idx: usize,
        data: usize,
        offset: usize,
        size: usize,
    ) -> bool {
        if !self.census_armed || self.set.classifier_mode {
            return false;
        }
        let Some(mut block) = take(data) else {
            return false;
        };
        if block.extent != offset
            || block.size != size
            || crate::arena::pointer_in_nursery(data + GC_HEADER_SIZE)
        {
            return false;
        }
        // Extent/generation checks above establish that the recorded starts
        // still describe this block. No mutator ran; only trace marks changed.
        block.refresh_traced_header_facts(data);
        GC_STATS.with(|stats| {
            let mut stats = stats.borrow_mut();
            stats.adopted_blocks = stats.adopted_blocks.saturating_add(1);
        });
        if block.objects == 0 {
            // The census opens no block for one without a walkable object.
            return true;
        }
        #[cfg(test)]
        let recorded = (
            block.chunks.clone(),
            block.objects,
            block.bytes,
            block.first_start,
            block.last_start,
            block.non_walkable,
            matches!(block.liveness, crate::arena::PromotionLiveness::Marked),
        );
        self.census_block_idx = block_idx;
        self.set.begin_arena_block_with_chunks(
            u32::try_from(block_idx).unwrap_or(u32::MAX),
            data,
            offset,
            block.words,
            block.chunks,
        );
        self.set.block_census.begin_block(block_idx, data, offset);
        self.set.block_census.note_whole_block_walk();
        self.set.block_census.adopt_block_facts(
            block.objects as u64,
            block.bytes,
            &block.types,
            block.flag_obligation,
            block.premarked,
            block.non_walkable,
        );
        self.set.arena_count += block.objects;
        self.set.record_pointer_range(block.first_start);
        self.set.record_pointer_range(block.last_start);
        #[cfg(test)]
        if !sabotage::dropping_starts() {
            verify_adopted_block(block_idx, data, offset, size, &self.set, recorded);
        }
        true
    }
}

/// Test builds: walk the adopted block with the census walk into a scratch set
/// and assert the adopted record says the same.
#[cfg(test)]
unsafe fn verify_adopted_block(
    block_idx: usize,
    data: usize,
    offset: usize,
    size: usize,
    adopted: &ValidPointerSet,
    recorded: (Vec<Vec<u64>>, usize, u64, usize, usize, bool, bool),
) {
    let mut scratch = ValidPointerSetBuilder::new();
    scratch.walk_census_block(block_idx, data, offset, size);
    scratch.set.block_census.flush_block();
    let (chunks, objects, bytes, first, last, non_walkable, traced) = recorded;
    let walked_block = scratch
        .set
        .arena_blocks
        .last()
        .expect("the walk opened the block");
    assert_eq!(walked_block.base, data);
    assert_eq!(scratch.set.arena_count, objects, "adopted object count");
    assert_eq!(
        scratch.set.start_bitmap_chunks, chunks,
        "adopted start bitmap of block {data:#x}"
    );
    assert_eq!(
        (scratch.set.range_min, scratch.set.range_max),
        (first, last)
    );
    let walked = scratch
        .set
        .block_census
        .block(block_idx)
        .expect("walked facts");
    assert_eq!((walked.objects, walked.bytes), (objects as u64, bytes));
    assert_eq!(
        walked.non_walkable, non_walkable,
        "adopted non-walkable fact"
    );
    let adopted_facts = adopted.block_census.current_facts_for_tests();
    if traced {
        assert_eq!(
            adopted_facts.obligation, walked.obligation,
            "traced census must retain exact current sweep obligations"
        );
        assert_eq!(
            adopted_facts.premarked, walked.premarked,
            "cleared trace marks must not disable whole-block reclamation"
        );
    }
    assert!(
        adopted_facts.obligation || !walked.obligation,
        "an adopted record may only be more conservative than the walk (obligation)"
    );
    assert!(
        adopted_facts.premarked || !walked.premarked,
        "an adopted record may only be more conservative than the walk (pre-marked)"
    );
}

/// Sabotage switch for the adoption tests: records keep no start bits, and the
/// test-build verification stands down. Test builds only.
#[cfg(test)]
pub(crate) mod sabotage {
    use std::cell::Cell;

    thread_local! {
        static DROP_STARTS: Cell<bool> = const { Cell::new(false) };
    }

    pub(crate) fn dropping_starts() -> bool {
        DROP_STARTS.with(Cell::get)
    }

    pub(crate) struct Guard(bool);

    impl Guard {
        pub(crate) fn arm() -> Self {
            Self(DROP_STARTS.with(|s| s.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            DROP_STARTS.with(|s| s.set(self.0));
        }
    }
}

#[cfg(test)]
#[path = "adopt_census_flag_tests.rs"]
mod flag_refresh_tests;
