//! The allocation-point trigger's "nothing is due" watermark (#10698).
//!
//! `gc_check_trigger` runs on every `gc_malloc` and every arena block fill,
//! and on almost every call the answer is "nothing is due". Reaching that
//! answer re-derives four arms — old-gen reclaim, whole arena, young-gen
//! scavenge cap, malloc count — from a dozen thread-locals, a `RefCell`
//! borrow and the from-space occupancy read: ~550 instructions per call,
//! measured under callgrind, to evaluate a handful of `quantity >= threshold`
//! comparisons that are mostly nowhere near firing.
//!
//! # What the watermark holds
//!
//! Between two calls, only two of the ladder's quantities move without a
//! runtime write the collector controls:
//!
//! * the malloc registry's length — every `gc_malloc` pushes one; and
//! * the current Eden block's bump offset — the compiled inline allocator
//!   advances it without entering the runtime.
//!
//! So when a full evaluation answers "nothing is due" it publishes the two
//! bounds those quantities must stay under for the answer to still hold —
//! [`TriggerWatermark::malloc_limit`] and
//! [`TriggerWatermark::young_offset_limit`] — and the next call answers from
//! one load of each ([`nothing_due`]). Neither bound is an estimate: each is
//! the threshold the ladder itself compared against, re-expressed in the unit
//! the fast path reads.
//!
//! # Why everything else cannot move underneath it
//!
//! Every other input the ladder reads is held fixed by construction:
//!
//! * The thresholds, flags and counters are [`TriggerInput`] cells, and every
//!   write to one retires the watermark. It is a type, not a list of call
//!   sites, so a writer cannot be forgotten: a `Cell` method that does not
//!   retire it does not exist on the type, and the build fails at the site.
//! * The young generation's *sealed* bytes — everything but the current Eden
//!   block — change only across a heap-generation advance or an Eden block
//!   switch. Those are exactly the two events that retire
//!   `arena::from_space`'s sealed-bytes cache, and both retire this too
//!   (`heap_generation::advance`, `Arena::set_current`).
//! * Budgeted-cycle activity and the root lock are `TriggerInput`s as well:
//!   the watermark is only published with no cycle active and no root lock
//!   held, and entering either retires it, so the calls that would have
//!   stepped a cycle or recorded a deferred check always evaluate.
//! * The remaining entry guards — a budgeted step in progress, suppression,
//!   mid-allocation, an unsafe FFI zone — make `gc_check_trigger` return
//!   without acting, which is also what the fast path does.
//!
//! What the fast path skips is therefore exactly a call that would have
//! returned without acting, and it skips nothing else. Under `cfg(test)`
//! every fast-path answer is re-derived by the full ladder and asserted equal
//! (`verify_nothing_due`), so a writer that escaped the type — an `unsafe`
//! store, a new quantity the ladder starts reading — fails the runtime suite
//! instead of silently postponing a collection.
//!
//! # The compiled inline bump sequence is untouched
//!
//! The issue's constraint (#10377's lesson): nothing here adds an instruction
//! to the allocation sequence codegen emits. The young arm is read, not
//! trip-wired — the fast path loads the inline state's offset, which the
//! compiled allocator already maintains.

use std::cell::Cell;

/// A trigger-ladder input: a `Cell` whose every write retires the
/// allocation-point watermark.
///
/// Deliberately not `Deref<Target = Cell<T>>`: that would hand out `Cell`'s
/// own `set`/`replace`/`take`, which write without retiring.
#[repr(transparent)]
pub(crate) struct TriggerInput<T>(Cell<T>);

impl<T: Copy> TriggerInput<T> {
    pub(crate) const fn new(value: T) -> Self {
        Self(Cell::new(value))
    }

    #[inline(always)]
    pub(crate) fn get(&self) -> T {
        self.0.get()
    }

    #[inline]
    pub(crate) fn set(&self, value: T) {
        self.0.set(value);
        retire_trigger_watermark();
    }

    #[inline]
    pub(crate) fn replace(&self, value: T) -> T {
        let previous = self.0.replace(value);
        retire_trigger_watermark();
        previous
    }
}

/// The bounds under which the last full evaluation's "nothing is due" still
/// holds. See the module docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TriggerWatermark {
    /// The malloc-count arm's threshold (`GC_NEXT_MALLOC_TRIGGER`): nothing
    /// is due while the registry holds fewer objects than this. `0` — never
    /// a threshold the ladder answers "nothing due" against, since every
    /// count is `>= 0` — is the retired state.
    malloc_limit: usize,
    /// The young-cap arm's threshold minus the sealed young bytes, i.e. the
    /// bound on the inline allocator's offset in the current Eden block.
    /// `usize::MAX` while the nursery cap is not in force.
    young_offset_limit: usize,
}

impl TriggerWatermark {
    pub(super) const RETIRED: Self = Self {
        malloc_limit: 0,
        young_offset_limit: 0,
    };

    /// An arbitrary watermark, for the test that proves the verifier can fail.
    #[cfg(test)]
    pub(crate) const fn for_test(malloc_limit: usize, young_offset_limit: usize) -> Self {
        Self {
            malloc_limit,
            young_offset_limit,
        }
    }

    /// The watermark for a ladder evaluation that found nothing due.
    ///
    /// `young` is `None` when the young arm did not read the young
    /// generation (the nursery cap is not in force). Otherwise it carries the
    /// from-space occupancy the arm compared, the cap it compared it against
    /// (already lowered to the census-seed point while the census is
    /// unseeded), and the inline offset the occupancy was read through —
    /// `None` when the inline allocator was not initialized, in which case no
    /// fast path is published.
    pub(super) fn after_nothing_due(malloc_limit: usize, young: Option<YoungArmReading>) -> Self {
        let young_offset_limit = match young {
            None => usize::MAX,
            Some(YoungArmReading {
                from_space_in_use,
                limit,
                inline_offset: Some(inline_offset),
            }) => {
                // `from_space_in_use` is the sealed bytes plus this offset
                // (`copying_from_space_in_use_bytes` reads the current block
                // through the inline state whenever it is initialized), so
                // `offset < limit - sealed` iff `sealed + offset < limit`.
                let Some(sealed) = from_space_in_use.checked_sub(inline_offset) else {
                    return Self::RETIRED;
                };
                limit.saturating_sub(sealed)
            }
            Some(YoungArmReading {
                inline_offset: None,
                ..
            }) => return Self::RETIRED,
        };
        Self {
            malloc_limit,
            young_offset_limit,
        }
    }
}

/// What the young-cap arm read, for [`TriggerWatermark::after_nothing_due`].
#[derive(Clone, Copy, Debug)]
pub(super) struct YoungArmReading {
    pub(super) from_space_in_use: usize,
    pub(super) limit: usize,
    pub(super) inline_offset: Option<usize>,
}

crate::perry_thread_local! {
    static GC_TRIGGER_WATERMARK: Cell<TriggerWatermark> =
        const { Cell::new(TriggerWatermark::RETIRED) };
}

#[cfg(test)]
crate::perry_thread_local! {
    /// Fast-path answers given on this thread, each one re-derived by the
    /// ladder in [`verify_nothing_due`]. Lets a test assert the fast path was
    /// taken at all, not merely that nothing it did was wrong.
    static FAST_PATH_HITS: Cell<u64> = const { Cell::new(0) };
}

/// Forget the published watermark; the next `gc_check_trigger` evaluates
/// the full ladder. `try_with` because block releases and heap changes can
/// run while this thread's locals are being destroyed.
///
/// A thread whose hot-TLS cache is not filled yet has published nothing —
/// publishing resolves the cache first — so there is nothing to retire, and
/// resolving the watermark here would fill the cache from inside the
/// initializer that is writing (`Arena::new`'s `ARENA_TOTAL_BYTES`, which
/// runs inside the fill): unbounded recursion. See `tls_hot::hot_if_filled`.
///
/// Out of line: the writers include `Arena::try_block_alloc`'s old-gen arm,
/// and inlining this there grew that function past the point where it is
/// inlined into the allocation path (measured: +19 instructions per Eden
/// allocation, none of them in the old-gen arm).
#[inline(never)]
pub(crate) fn retire_trigger_watermark() {
    if crate::tls_hot::hot_if_filled().is_none() {
        return;
    }
    let _ = GC_TRIGGER_WATERMARK.try_with(|w| w.set(TriggerWatermark::RETIRED));
}

pub(super) fn publish_trigger_watermark(watermark: TriggerWatermark) {
    GC_TRIGGER_WATERMARK.with(|w| w.set(watermark));
}

/// The fast path: does the last published "nothing is due" still hold?
///
/// Two quantities, read live, against the bounds the ladder published.
#[inline(always)]
pub(super) fn nothing_due() -> bool {
    let watermark = GC_TRIGGER_WATERMARK.with(Cell::get);
    // SAFETY: an unguarded shared read of this thread's registry. It fails
    // (and the full ladder runs) only while a `borrow_mut` is live, where the
    // ladder's own `borrow()` would panic exactly as it did before this path
    // existed.
    let malloc_count = super::MALLOC_STATE.with(|state| unsafe {
        state
            .try_borrow_unguarded()
            .map_or(usize::MAX, |state| state.objects.len())
    });
    if malloc_count >= watermark.malloc_limit {
        return false;
    }
    // SAFETY: this thread's own inline allocator state; nothing holds a
    // `&mut` to it across this read.
    let inline = unsafe { &*crate::arena::hot_inline_state() };
    !inline.data.is_null() && inline.offset < watermark.young_offset_limit
}

/// `cfg(test)`: the fast path's answer must be the ladder's answer.
///
/// Called on every fast-path hit in the unit suite. A watermark that
/// outlived a change to something the ladder reads — a writer that escaped
/// [`TriggerInput`], a quantity the ladder newly depends on — surfaces here,
/// at the call that would have skipped a due collection.
#[cfg(test)]
pub(super) fn verify_nothing_due() {
    FAST_PATH_HITS.with(|hits| hits.set(hits.get() + 1));
    let (due, _) = super::policy::gc_budgeted_due_trigger_eval();
    assert!(
        due.is_none(),
        "#10698: the trigger watermark answered 'nothing due' while the ladder \
         says {due:?} is due -- an input changed without retiring it"
    );
}

/// How many `gc_check_trigger` calls on this thread the fast path answered.
#[cfg(test)]
pub(crate) fn trigger_watermark_fast_path_hits() -> u64 {
    FAST_PATH_HITS.with(Cell::get)
}

/// Test view of the published watermark.
#[cfg(test)]
pub(crate) fn published_trigger_watermark() -> Option<(usize, usize)> {
    let watermark = GC_TRIGGER_WATERMARK.with(Cell::get);
    (watermark != TriggerWatermark::RETIRED)
        .then_some((watermark.malloc_limit, watermark.young_offset_limit))
}

/// Hot-cache slot claimed by the watermark, which the fast path reads on
/// every `gc_malloc`. Liveness instrumentation for
/// `gc::tests::trigger_path_tls`.
#[cfg(test)]
pub(crate) fn trigger_watermark_slot_index() -> u32 {
    GC_TRIGGER_WATERMARK.slot_index()
}
