//! Monotone "has this feature ever been used?" latches for side-table probes.
//!
//! The runtime answers "is this value special?" for a large number of exotic
//! kinds — typed array, `Buffer`, `SharedArrayBuffer` backing, `Symbol`,
//! `DataView`, `ArrayBuffer`, `Map`, `Set` — by consulting an address-keyed
//! side table. Those probes sit on *generic* paths (property get/set, element
//! access, `typeof`, coercion, `JSON.stringify`, console formatting, GC header
//! reads), so a program pays for every kind it does not use, on every value it
//! touches. Each probe costs at minimum a thread-local resolution (on Darwin
//! that is a real call through `_tlv_get_addr` — there is no local-exec TLS)
//! plus a `RefCell` borrow and a hash, and for the process-global tables a
//! mutex acquisition.
//!
//! Symbolicated profiles of two unrelated realistic programs measured **13% of
//! total runtime** in exactly these probes, for features neither program used:
//! an async service pipeline spent 2.45% in `lookup_typed_array_kind`, 2.40% in
//! `is_registered_buffer`, 1.22% in `is_shared_sab` and 0.71% in
//! `is_registered_symbol` while allocating no typed array, no `Buffer`, no
//! `SharedArrayBuffer` and no `Symbol`; a tree-walking interpreter showed the
//! same two leaders independently.
//!
//! [`RegistryLatch`] removes that tax. It is a process-global `AtomicBool` that
//! starts `false` and is armed by the *registration* site. A probe checks it
//! first and answers "no" from a single atomic load when the feature has never
//! been used. #7474 established the pattern for `map`/`set`; this type
//! generalises it so the remaining probes get it without copy-paste.
//!
//! # Why monotone
//!
//! The latch has no `disarm`, deliberately — there is no counter to get wrong
//! and no ordering hazard between an unregister and a concurrent probe. Once
//! armed the process pays the ordinary slow path forever, which is merely
//! slower, never wrong. That asymmetry is the whole safety argument: the only
//! *incorrect* observation this design can produce is `idle` while a table is
//! non-empty. `armed` while every table is empty is free of consequence.
//!
//! # The ordering rule (binding)
//!
//! **`arm()` must be called BEFORE the registry mutation it advertises, in the
//! registering thread's program order.** Arming *after* the insert opens a
//! window in which the feature is live and reachable but the latch still reads
//! idle, so a concurrent probe takes the fast path and answers `false` for an
//! address that is genuinely registered. That is not hypothetical: the sibling
//! latch `buffer::header::EXTERNAL_BUFFERS_NONEMPTY` carries an inline comment
//! for precisely this reason, and `js_buffer_register_external` latches first.
//!
//! With the arm placed first, the argument for a reader on *another* thread is:
//! a thread can only probe an address it holds, and every route by which an
//! address reaches a different thread in this runtime passes through a
//! synchronising edge (the `SerializedValue` deep-copy queue and the
//! `PENDING_THREAD_RESULTS` drain are both mutex/channel mediated). The arm
//! precedes the registration, which precedes the hand-off, so the arm is in the
//! reader's happens-before past and the reader must observe it. The
//! thread-local tables (`BUFFER_REGISTRY`, `TYPED_ARRAY_REGISTRY`,
//! `UINT8ARRAY_FROM_CTOR`) need even less: only the arming thread can find
//! their entries at all, and a thread always observes its own prior store.
//!
//! `Acquire`/`Release` is therefore stronger than today's routes require —
//! `Relaxed` would be sound given the hand-off edges above. It costs one
//! instruction and removes the need to re-audit this file the next time someone
//! publishes a heap address through a lock-free path, so the stronger ordering
//! is what ships.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

/// A one-way "this feature has been registered at least once" flag.
///
/// See the module docs for the ordering rule: arm before you publish.
#[derive(Debug, Default)]
pub struct RegistryLatch {
    armed: AtomicBool,
}

impl RegistryLatch {
    /// A latch that has never been armed.
    pub const fn new() -> Self {
        Self {
            armed: AtomicBool::new(false),
        }
    }

    /// True while nothing has ever been registered, so an address-keyed probe
    /// over the guarded table can answer "not found" without touching it.
    ///
    /// This is the hot side: one relaxed-cost atomic load in place of a
    /// thread-local resolution plus a hash probe (plus, for the global tables,
    /// a mutex acquisition).
    #[inline(always)]
    pub fn is_idle(&self) -> bool {
        !self.armed.load(Ordering::Acquire)
    }

    /// True once anything has ever been registered. Never goes back to false.
    #[inline(always)]
    pub fn is_armed(&self) -> bool {
        self.armed.load(Ordering::Acquire)
    }

    /// Publish "this feature is now in use".
    ///
    /// MUST run **before** the guarded table is mutated — see the module docs.
    ///
    /// The load-then-store keeps a hot registration loop from re-dirtying a
    /// shared cache line on every allocation once the latch is already armed;
    /// skipping the store is sound because some earlier `Release` store already
    /// established the value, and the latch never travels back to `false`.
    #[inline]
    pub fn arm(&self) {
        if !self.armed.load(Ordering::Relaxed) {
            self.armed.store(true, Ordering::Release);
        }
    }
}

/// A monotone "smallest and largest address ever registered" window.
///
/// [`RegistryLatch`] answers "has this feature EVER been used?". That question
/// stops discriminating the moment a program registers its first entry — and
/// for the two hottest probes in the runtime it stops discriminating almost
/// immediately: a `claude-code --help` run registers **10** buffers and **42**
/// typed arrays, then probes `is_registered_buffer` 4,650,058 times and
/// `lookup_typed_array_kind` 3,566,956 times. Counted with uretprobes on that
/// binary, the buffer probe answered `true` **4** times and the typed-array
/// probe answered `Some` **zero** times. The latch was armed for every one of
/// those calls, so all 8.2 M paid the out-of-line call, a thread-local
/// resolution, a `RefCell` borrow and a hash to say "no".
///
/// This window is the same monotone idea applied to the address instead of to
/// the fact of registration: every registration widens `[lo, hi]` *before* it
/// publishes, so an address outside the window cannot be in any table the
/// window covers. Rejecting is therefore sound; accepting merely falls through
/// to the exact lookup that was already there.
///
/// It is strictly stronger than a latch — an unregistered process has the empty
/// window `[usize::MAX, 0]`, which contains nothing — and it costs two adjacent
/// static loads and two compares, which inline into the probe's call sites
/// instead of being paid behind a call.
///
/// # The ordering rule (binding, and identical to [`RegistryLatch`]'s)
///
/// **[`admit`](Self::admit) must run BEFORE the registry mutation it
/// advertises.** Widening after the insert opens a window in which an address
/// is registered but outside the published range, so a concurrent probe would
/// answer `false` for an address that is genuinely registered.
///
/// `lo` and `hi` are separate atomics, so a racing reader can observe a mix of
/// old and new values. That is harmless: each moves in one direction only, so
/// once `lo <= a <= hi` holds for an address it holds forever, and a reader
/// that observes a partially-updated pair observes a window that is only ever
/// wider than the one it replaced — never narrower.
///
/// Cross-thread visibility rests on [`admit`](Self::admit)'s `AcqRel`
/// read-modify-writes. An RMW reads the latest value in its location's
/// modification order, so its acquire half joins every earlier widening — by
/// any thread — into the admitting thread's happens-before graph before that
/// thread publishes the address. A reader can only ask about an address it has
/// obtained, which requires the publishing hand-off, and `may_contain`'s
/// `Acquire` loads complete the chain. This is why `admit` may not skip the
/// RMW: see its own documentation.
#[derive(Debug)]
pub struct RegistryAddrWindow {
    lo: AtomicUsize,
    hi: AtomicUsize,
}

impl Default for RegistryAddrWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl RegistryAddrWindow {
    /// An empty window: contains no address at all.
    pub const fn new() -> Self {
        Self {
            lo: AtomicUsize::new(usize::MAX),
            hi: AtomicUsize::new(0),
        }
    }

    /// `false` ⟹ `addr` is definitively absent from every table this window
    /// covers, so the caller can answer "not found" without touching one.
    ///
    /// This is the hot side. It is deliberately `inline(always)`: the whole
    /// point is that the common negative answer costs a couple of loads at the
    /// call site rather than a call into the registry probe.
    #[inline(always)]
    pub fn may_contain(&self, addr: usize) -> bool {
        addr >= self.lo.load(Ordering::Acquire) && addr <= self.hi.load(Ordering::Acquire)
    }

    /// Widen the window to include `addr`.
    ///
    /// MUST run **before** the guarded table is mutated — see the type docs.
    ///
    /// Two unconditional atomic read-modify-writes, deliberately with no
    /// "already covered?" pre-check in front of them. Registration is rare —
    /// `claude-code --help` calls this 10 times for buffers and 42 times for
    /// typed arrays across a 6.9-billion-instruction run — so a fast path here
    /// buys nothing measurable and costs the one thing this type cannot spend:
    /// certainty. Two earlier drafts of exactly this function were wrong.
    ///
    /// 1. `load` then `store` is a read-modify-write with a hole in it. Two
    ///    threads registering at once both read the old bound, and the
    ///    *narrower* of the two stores can land last — dropping the other
    ///    thread's address out of the window while its entry is live in that
    ///    thread's registry. `fetch_min`/`fetch_max` are single RMWs, so no
    ///    update can be lost no matter how the two interleave.
    ///
    /// 2. A `Relaxed` "skip if already covered" pre-check reintroduces the same
    ///    bug one level up, in the memory model rather than in the interleaving.
    ///    If this thread skips the RMW because it *observed* another thread's
    ///    widening, it performs no acquire, so that other thread's widening
    ///    never enters this thread's happens-before graph — and a reader that
    ///    synchronises only with THIS thread's subsequent publish is not
    ///    guaranteed to see the bound that actually covers the address. It
    ///    would answer "not registered" for a registered pointer.
    ///
    /// `AcqRel` closes that: an RMW reads the latest value in the location's
    /// modification order, and the acquire half joins every prior widening into
    /// this thread's happens-before graph before the caller publishes.
    #[inline]
    pub fn admit(&self, addr: usize) {
        self.lo.fetch_min(addr, Ordering::AcqRel);
        self.hi.fetch_max(addr, Ordering::AcqRel);
    }

    /// The live `[lo, hi]` pair, or `None` while the window is still empty.
    ///
    /// Diagnostic use: `PERRY_BUFFER_DIAG` reports it, so a window that has
    /// widened until it covers the heap is visible rather than inferred.
    pub(crate) fn bounds(&self) -> Option<(usize, usize)> {
        let lo = self.lo.load(Ordering::Acquire);
        let hi = self.hi.load(Ordering::Acquire);
        (lo <= hi).then_some((lo, hi))
    }

    /// Test hook: the current `[lo, hi]` pair, or `None` while empty.
    #[cfg(test)]
    pub(crate) fn bounds_for_tests(&self) -> Option<(usize, usize)> {
        self.bounds()
    }
}

/// A monotone "which addresses have ever been registered?" **set filter** —
/// a Bloom filter over the registered addresses, for the probes a
/// [`RegistryAddrWindow`] cannot discriminate.
///
/// # Why a second shape was needed
///
/// The window works when the registered addresses sit in a narrow band. Two of
/// the probes measured after #9272 do not: their entries are ordinary GC-heap
/// objects, interleaved with everything else the program allocates, so `[lo,
/// hi]` grows to cover most of the heap and stops rejecting. Measured on
/// `claude-code --help` by replaying each probe's real argument stream against
/// the window the registrations would have built:
///
/// | probe | calls | a window rejects | this filter rejects |
/// |---|---|---|---|
/// | `is_registered_symbol` | 378,163 | 38.3% | **99.58%** |
/// | `is_registered_class_prototype_object` | 26,290 | 54.0% | **99.05%** |
///
/// (`is_uint8array_buffer`, whose entries are `BufferHeader`s, is the opposite
/// case: a window rejects **100%** of its 540,328 calls, so it keeps the
/// cheaper shape. Both are in the tree on purpose; pick by measurement.)
///
/// # The contract, which is the window's contract
///
/// `may_contain` returning `false` means "definitively not registered". A Bloom
/// filter has false positives and no false negatives, which is exactly the
/// asymmetry the probes need: a false positive costs the ordinary lookup that
/// was already there, and a false negative — the one dangerous answer — cannot
/// occur while every registration sets its bits before it publishes.
///
/// Bits are only ever SET, never cleared, so unregistration (death pruning, the
/// GC's dead-buffer sweep) leaves the filter a weaker approximation, never a
/// wrong one. There is deliberately no `clear`.
///
/// # Sizing
///
/// 1,024 bits (16 `AtomicU64`, two cache lines) and three probes per address.
/// `claude-code --help` registers **100** symbols and **160** class prototypes,
/// so at 160 entries the theoretical false-positive rate is
/// `(1 - e^(-3·160/1024))³ ≈ 1.6%`; the measured rates on the real address
/// streams were 0.26% and 0.49%.
///
/// **The saturation regime is deliberate, and it is the reason `WORDS` is the
/// only knob.** `CLASS_PROTOTYPE_OBJECTS` grows by one entry per ES5-transpiled
/// constructor (#9225), so a bundle much larger than claude-code's can push it
/// past ~700 entries, where 1,024 bits saturate and `may_contain` starts
/// answering `true` for almost everything. That is not a correctness cliff and
/// not a regression: a saturated filter is exactly the code that ran before it
/// existed — the probe falls through to the lookup it always did. It is a
/// *win* cliff. If a corpus is found sitting on the wrong side of it, raise
/// `WORDS`: 64 words (4,096 bits, 512 B) holds ~640 entries at the same
/// false-positive rate and is still trivially L1-resident. The value shipped
/// here is the one the measurements above were taken at, and is deliberately
/// not raised on speculation.
///
/// `WORDS` is a per-owner parameter, defaulting to 16. Issue #10110 measured
/// 643–648 live canonical wrappers saturating this default on a separate
/// campaign branch. That owner needs 256 words and [`RegistryAddrIndex`] to
/// release retired addresses; increasing a monotone filter only postpones
/// saturation. The canonical-wrapper subsystem has not landed on `main` yet.
///
/// **Bits accrue per ADMISSION, not per live entry.** Both tables this guards
/// are re-keyed by the collector — a symbol or a prototype that is evacuated is
/// admitted again at its new address, and the bits its old address set are
/// never cleared — so a long-running program with a moving nursery walks
/// towards saturation over time rather than sitting at a fixed occupancy. That
/// is measurable rather than theoretical, and it was measured: re-running the
/// answer census on the shipped `claude --help` binary, the symbol filter ended
/// the run admitting 1,492 of 378,163 probes, of which 622 were genuine — 870
/// false positives, a 0.23% rate against a population that had been evacuated
/// and re-admitted throughout. The bound to watch is the false-positive rate at
/// END of run, not the registration count.
///
/// # The ordering rule (binding, and identical to [`RegistryAddrWindow`]'s)
///
/// **[`admit`](Self::admit) must run BEFORE the registry mutation it
/// advertises.** Setting the bits after the insert opens a window in which an
/// address is registered but the filter denies it.
///
/// The three words are separate atomics, so a racing reader can observe a mix
/// of old and new. That is harmless for the same reason it is harmless for the
/// window: each word only ever gains bits, so once `may_contain` holds for an
/// address it holds forever, and a partially-observed filter is only ever a
/// weaker filter — never one that rejects something it used to accept.
///
/// Cross-thread visibility rests on `admit`'s `AcqRel` read-modify-writes, and
/// on `may_contain`'s `Acquire` loads, exactly as for the window. As there,
/// `admit` performs its RMWs unconditionally: a `Relaxed` "already set?"
/// pre-check would let a thread publish an address without ever acquiring
/// another thread's bit-setting, so a reader synchronising only with this
/// thread could miss it.
pub struct RegistryAddrFilter<const WORDS: usize = 16> {
    words: [AtomicU64; WORDS],
}

impl<const WORDS: usize> Default for RegistryAddrFilter<WORDS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const WORDS: usize> std::fmt::Debug for RegistryAddrFilter<WORDS> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegistryAddrFilter")
            .field("words", &WORDS)
            .field("bits_set", &self.bits_set())
            .finish()
    }
}

impl<const WORDS: usize> RegistryAddrFilter<WORDS> {
    const BITS: u64 = (WORDS as u64) * 64;
    const HASH_BITS: u32 = Self::BITS.ilog2();

    /// An empty filter: contains no address at all.
    pub const fn new() -> Self {
        // Three disjoint slices must fit in the 64-bit hash. Power-of-two
        // widths keep the hot probes division-free after constant folding.
        assert!(WORDS.is_power_of_two() && WORDS <= (1 << 15));
        Self {
            words: [const { AtomicU64::new(0) }; WORDS],
        }
    }

    /// The three bit positions for `addr`.
    ///
    /// Registered addresses are allocator results, so the low three bits carry
    /// no information and are shifted out; the multiply then spreads what is
    /// left across the whole word, and the three slices are taken from the top,
    /// where a 64-bit multiply mixes best. Deliberately branch-free and
    /// division-free — this runs inline at every call site of the probes it
    /// guards.
    #[inline(always)]
    const fn bit_positions(addr: usize) -> (u64, u64, u64) {
        let h = ((addr as u64) >> 3).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        (
            (h >> (64 - Self::HASH_BITS)) % Self::BITS,
            (h >> (64 - 2 * Self::HASH_BITS)) % Self::BITS,
            (h >> (64 - 3 * Self::HASH_BITS)) % Self::BITS,
        )
    }

    #[inline(always)]
    fn bit_is_set(&self, bit: u64) -> bool {
        let word = self.words[(bit / 64) as usize].load(Ordering::Acquire);
        word & (1u64 << (bit % 64)) != 0
    }

    /// `false` ⟹ `addr` is definitively absent from every table this filter
    /// covers, so the caller can answer "not found" without touching one.
    ///
    /// This is the hot side, and `inline(always)` for the same reason the
    /// window's is: the point is that the common negative answer costs a load
    /// and a test at the call site rather than a call into the registry probe.
    /// The `&&` chain short-circuits, so most rejections read one word.
    #[inline(always)]
    pub fn may_contain(&self, addr: usize) -> bool {
        let (a, b, c) = Self::bit_positions(addr);
        self.bit_is_set(a) && self.bit_is_set(b) && self.bit_is_set(c)
    }

    /// Add `addr` to the filter.
    ///
    /// MUST run **before** the guarded table is mutated — see the type docs.
    #[inline]
    pub fn admit(&self, addr: usize) {
        let (a, b, c) = Self::bit_positions(addr);
        for bit in [a, b, c] {
            self.words[(bit / 64) as usize].fetch_or(1u64 << (bit % 64), Ordering::AcqRel);
        }
    }

    /// Replace every word with the bits of `live`.
    ///
    /// **Only [`RegistryAddrIndex`] may call this**, and only while holding the
    /// guard that serializes this filter's admissions: the soundness argument
    /// is written out on that type, and it depends on `live` being the complete
    /// current population and on no `admit` running concurrently.
    fn overwrite_from_live(&self, live: impl IntoIterator<Item = usize>) {
        let mut fresh = [0u64; WORDS];
        for addr in live {
            let (a, b, c) = Self::bit_positions(addr);
            for bit in [a, b, c] {
                fresh[(bit / 64) as usize] |= 1u64 << (bit % 64);
            }
        }
        // Release, so a reader that observes a word observes the set membership
        // that produced it. Word-at-a-time publication is safe because every
        // live address's bits are present in both the old and the new value.
        for (word, value) in self.words.iter().zip(fresh.iter()) {
            word.store(*value, Ordering::Release);
        }
    }

    /// How many bits this filter has in total. Diagnostics and tests only:
    /// an occupancy count means nothing without the capacity it is a share of.
    pub fn capacity_bits(&self) -> u32 {
        Self::BITS as u32
    }

    /// How many bits are set. Diagnostics and tests only: a filter whose bits
    /// are nearly all set has stopped discriminating, and a test that wants to
    /// prove the fast path RAN needs to know the filter is not saturated.
    pub fn bits_set(&self) -> u32 {
        self.words
            .iter()
            .map(|w| w.load(Ordering::Acquire).count_ones())
            .sum()
    }

    /// Test hook: empty the filter, so a test can establish a state in which
    /// only what it registers is admitted.
    ///
    /// Only a test may have this: the soundness argument is that bits are never
    /// cleared, so a production clear would make live registered addresses read
    /// as unregistered. A test that calls this must restore what it cleared —
    /// see [`Self::restore_for_tests`].
    #[cfg(test)]
    pub(crate) fn take_for_tests(&self) -> [u64; WORDS] {
        let mut previous = [0u64; WORDS];
        for (slot, word) in previous.iter_mut().zip(self.words.iter()) {
            *slot = word.swap(0, Ordering::AcqRel);
        }
        previous
    }

    /// Test hook: OR the saved bits back in.
    #[cfg(test)]
    pub(crate) fn restore_for_tests(&self, saved: [u64; WORDS]) {
        for (word, bits) in self.words.iter().zip(saved.iter()) {
            word.fetch_or(*bits, Ordering::AcqRel);
        }
    }

    /// Test hook: the current bit words, as a plain value.
    #[cfg(test)]
    pub(crate) fn snapshot_for_tests(&self) -> [u64; WORDS] {
        let mut words = [0u64; WORDS];
        for (slot, word) in words.iter_mut().zip(self.words.iter()) {
            *slot = word.load(Ordering::Acquire);
        }
        words
    }

    /// Test hook: what [`may_contain`](Self::may_contain) WOULD have answered
    /// for `addr` against a snapshot taken earlier.
    ///
    /// This is what lets a test prove a widening was load-bearing rather than
    /// lucky: "the address the collector moved this symbol to is not one the
    /// filter already happened to accept" is only checkable against the filter
    /// as it stood BEFORE the move.
    #[cfg(test)]
    pub(crate) fn snapshot_may_contain(words: &[u64; WORDS], addr: usize) -> bool {
        let (a, b, c) = Self::bit_positions(addr);
        [a, b, c]
            .into_iter()
            .all(|bit| words[(bit / 64) as usize] & (1u64 << (bit % 64)) != 0)
    }

    /// The word count, so a caller can name the snapshot type.
    #[cfg(test)]
    pub(crate) const fn words() -> usize {
        WORDS
    }
}

/// The addresses an owner currently holds, sized from that population and
/// narrowed when the population shrinks.
///
/// [`RegistryAddrFilter`] answers "was this address EVER admitted?", by default
/// in 1,024 bits. Both halves of that sentence are what the #10110 census
/// measured going wrong for the canonical-handle owner: 643–648 live addresses
/// do not fit in 1,024 bits, and the 712–721 admissions a single command makes
/// are never released, so even a filter sized for the live set would walk
/// towards saturation over a session. This type fixes both without changing the
/// hot side: `may_contain` is still the same three bit tests on a plain array of
/// atomics, with no lock, no allocation and no pointer chase.
///
/// # What it adds
///
/// The owner's live addresses are kept in an exact set, mutated only when a
/// wrapper is created or retired — 712–721 and 69–76 times per command
/// respectively, against tens of millions of probes, so the mutex that guards it
/// is never contended by the hot path. When enough has changed, the bit array is
/// recomputed from that exact set, which is what releases retired addresses.
/// Refreshes are paced by the live population, with no timer or workload knob.
///
/// # Why overwriting the bits is sound
///
/// [`RegistryAddrFilter`] deliberately has no `clear`, because clearing bits
/// could make a live registered address read as unregistered — the one
/// dangerous answer. The refresh here is not a clear:
///
/// * Every `admit` and every refresh for one index is serialized by the same
///   mutex, so no admission's bit-set can be lost to a refresh's store. This is
///   why `admit` takes the guard *before* setting the bits.
/// * The new words are computed from the complete live set. For an address that
///   is live, its three bits are therefore set in the new value; they are also
///   set in the old value, because it was admitted under this same guard and
///   every refresh published while it was live also included it. A reader may
///   observe any mix of old and new words, and in every such mix all three of a
///   live address's bits are set. **No reader can observe a false negative at
///   any instant**, which is the same guarantee the monotone filter gives.
/// * An address that is *not* live may start reading `false`. That is the point,
///   and it is safe because the owner removes an address from the live set only
///   after its authoritative registry no longer recognises it, and because the
///   index is only ever a gate in front of that authoritative lookup: a stale
///   `true` costs the lookup that was always there, exactly as a Bloom false
///   positive does.
///
/// # The ordering rule (binding, inherited unchanged)
///
/// [`admit`](Self::admit) must run BEFORE the registry mutation it advertises,
/// and [`retire`](Self::retire) AFTER the registry mutation that ends the
/// address's registration. Retiring first would open a window in which the
/// registry still recognises an address the index already denies.
///
/// This is a set, not a reference count: repeated admissions are idempotent.
/// The owner must serialize an address's complete registration/retirement
/// lifecycle, including reuse, so a delayed retirement cannot erase a newer
/// admission. As with the monotone filter, publishing an address to another
/// thread must establish a happens-before edge from its admission.
pub struct RegistryAddrIndex<const WORDS: usize = 16> {
    filter: RegistryAddrFilter<WORDS>,
    live: Mutex<Option<LiveAddrs>>,
}

struct LiveAddrs {
    addrs: crate::fast_hash::PtrHashSet<usize>,
    /// Admissions plus retirements since the last refresh. Only ever touched
    /// under the `live` guard.
    changes_since_refresh: usize,
}

/// A refresh costs O(live), so it may not run more often than once per O(live)
/// changes. This floor keeps a tiny population from refreshing on every single
/// change; it is a lower bound on work, not a tuning knob for a workload.
const REFRESH_CHANGE_FLOOR: usize = 64;

impl<const WORDS: usize> Default for RegistryAddrIndex<WORDS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const WORDS: usize> std::fmt::Debug for RegistryAddrIndex<WORDS> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegistryAddrIndex")
            .field("words", &WORDS)
            .field("bits_set", &self.bits_set())
            .field("live", &self.live_count())
            .finish()
    }
}

impl<const WORDS: usize> RegistryAddrIndex<WORDS> {
    /// An empty index: contains no address at all.
    pub const fn new() -> Self {
        Self {
            filter: RegistryAddrFilter::new(),
            live: Mutex::new(None),
        }
    }

    /// `false` ⟹ `addr` is definitively not registered with this owner.
    ///
    /// The hot side, unchanged from [`RegistryAddrFilter::may_contain`]: three
    /// bit tests, short-circuiting, no lock and no allocation.
    #[inline(always)]
    pub fn may_contain(&self, addr: usize) -> bool {
        self.filter.may_contain(addr)
    }

    fn guard(&self) -> MutexGuard<'_, Option<LiveAddrs>> {
        // A poisoned guard must not turn a classification into a panic: the
        // live set is only an accelerator, and its worst state is stale.
        self.live
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Add `addr`. MUST run before the registry mutation it advertises.
    pub fn admit(&self, addr: usize) {
        let mut guard = self.guard();
        let state = guard.get_or_insert_with(|| LiveAddrs {
            addrs: crate::fast_hash::new_ptr_hash_set(),
            changes_since_refresh: 0,
        });
        // Under the guard, so a concurrent refresh cannot drop these bits.
        self.filter.admit(addr);
        if state.addrs.insert(addr) {
            state.changes_since_refresh += 1;
        }
        self.refresh_if_warranted(state);
    }

    /// Drop `addr`. MUST run after the registry mutation that ends its
    /// registration, so no window exists in which the registry recognises an
    /// address this index already denies.
    pub fn retire(&self, addr: usize) {
        let mut guard = self.guard();
        let Some(state) = guard.as_mut() else {
            return;
        };
        if !state.addrs.remove(&addr) {
            return;
        }
        state.changes_since_refresh += 1;
        self.refresh_if_warranted(state);
    }

    fn refresh_if_warranted(&self, state: &mut LiveAddrs) {
        if state.changes_since_refresh < REFRESH_CHANGE_FLOOR.max(state.addrs.len()) {
            return;
        }
        state.changes_since_refresh = 0;
        self.filter.overwrite_from_live(state.addrs.iter().copied());
    }

    /// Bits set in the underlying array, and its capacity. Diagnostics: an
    /// occupancy count means nothing without the capacity it is a share of.
    pub fn occupancy(&self) -> (u32, u32) {
        (self.filter.bits_set(), self.filter.capacity_bits())
    }

    fn bits_set(&self) -> u32 {
        self.filter.bits_set()
    }

    /// How many addresses the owner currently holds.
    pub fn live_count(&self) -> usize {
        self.guard().as_ref().map_or(0, |state| state.addrs.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic wrapper addresses: 8-byte aligned, above the handle band,
    /// spread the way `gc_malloc` results are rather than densely packed.
    fn wrapper_addrs(count: usize) -> Vec<usize> {
        (0..count).map(|i| 0x7F00_0000_1000usize + i * 96).collect()
    }

    fn probe_addrs(count: usize) -> Vec<usize> {
        (0..count).map(|i| 0x6100_0000_0008usize + i * 64).collect()
    }

    fn pass_rate<const WORDS: usize>(index: &RegistryAddrIndex<WORDS>, probes: &[usize]) -> f64 {
        let passes = probes.iter().filter(|&&a| index.may_contain(a)).count();
        passes as f64 / probes.len() as f64
    }

    #[test]
    fn wider_filter_uses_the_full_width_for_each_probe() {
        let mut high_half_seen = [false; 3];
        for addr in wrapper_addrs(644) {
            let (a, b, c) = RegistryAddrFilter::<256>::bit_positions(addr);
            for (seen, bit) in high_half_seen.iter_mut().zip([a, b, c]) {
                assert!(bit < 16_384);
                *seen |= bit >= 8_192;
            }
        }
        assert!(high_half_seen.into_iter().all(|seen| seen));
    }

    #[test]
    fn index_supports_duplicate_admission_and_address_reuse() {
        let index: RegistryAddrIndex<256> = RegistryAddrIndex::new();
        let addr = wrapper_addrs(1)[0];
        index.retire(addr);
        assert_eq!(index.live_count(), 0);
        assert!(!index.may_contain(addr));

        for _ in 0..3 {
            index.admit(addr);
            index.admit(addr);
            assert_eq!(index.live_count(), 1);
            assert!(index.may_contain(addr));
            index.retire(addr);
            index.retire(addr);
            assert_eq!(index.live_count(), 0);
        }
        index.admit(addr);
        assert_eq!(index.live_count(), 1);
        assert!(index.may_contain(addr));
    }

    #[test]
    fn concurrent_refreshes_preserve_published_live_addresses() {
        use std::sync::Barrier;

        const WRITERS: usize = 4;
        const RESIDENT: usize = 64;
        let index: RegistryAddrIndex<256> = RegistryAddrIndex::new();
        let resident = wrapper_addrs(RESIDENT);
        for &addr in &resident {
            index.admit(addr);
        }
        let start = Barrier::new(WRITERS + 1);
        std::thread::scope(|scope| {
            for writer in 0..WRITERS {
                let (index, start, resident) = (&index, &start, &resident);
                scope.spawn(move || {
                    start.wait();
                    for i in 0..2_000 {
                        let addr = 0x6100_0000_0008 + (i * WRITERS + writer) * 64;
                        index.admit(addr);
                        assert!(index.may_contain(addr));
                        for &live in resident {
                            assert!(index.may_contain(live));
                        }
                        index.retire(addr);
                    }
                });
            }
            start.wait();
            for _ in 0..2_000 {
                for &addr in &resident {
                    assert!(index.may_contain(addr));
                }
            }
        });
        assert_eq!(index.live_count(), RESIDENT);
        for addr in resident {
            assert!(index.may_contain(addr));
        }
        assert!(pass_rate(&index, &probe_addrs(20_000)) < 0.02);
    }

    /// The 644-address population reported in #10110 must fit in the proposed
    /// canonical-owner width. The default width is a saturation witness.
    #[test]
    fn sized_index_holds_the_measured_live_population() {
        const LIVE: usize = 644;
        let live = wrapper_addrs(LIVE);
        let probes = probe_addrs(20_000);

        let sized: RegistryAddrIndex<256> = RegistryAddrIndex::new();
        for &addr in &live {
            sized.admit(addr);
        }
        assert_eq!(sized.live_count(), LIVE);
        let sized_rate = pass_rate(&sized, &probes);
        assert!(
            sized_rate < 0.02,
            "sized index admits {sized_rate} of non-members"
        );
        for &addr in &live {
            assert!(sized.may_contain(addr), "sized index lost a live address");
        }

        let small: RegistryAddrIndex<16> = RegistryAddrIndex::new();
        for &addr in &live {
            small.admit(addr);
        }
        let small_rate = pass_rate(&small, &probes);
        assert!(
            small_rate > 0.30,
            "the 1,024-bit witness should be saturated by this population, got {small_rate}"
        );
        assert!(
            sized_rate * 10.0 < small_rate,
            "{sized_rate} vs {small_rate}"
        );
    }

    /// Retirement must actually narrow the index, and must never drop a live
    /// address while doing it. A refresh that kept retired addresses would make
    /// this type no better than the monotone filter.
    #[test]
    fn refresh_releases_retired_addresses_and_keeps_live_ones() {
        let addrs = wrapper_addrs(600);
        let index: RegistryAddrIndex<256> = RegistryAddrIndex::new();
        for &addr in &addrs {
            index.admit(addr);
        }
        let (occupied, capacity) = index.occupancy();
        assert!(occupied > 0 && occupied < capacity);

        let (live, retired) = addrs.split_at(50);
        for &addr in retired {
            index.retire(addr);
        }
        assert_eq!(index.live_count(), live.len());
        let (after, _) = index.occupancy();
        assert!(
            after * 4 < occupied,
            "occupancy {after} should collapse towards the live set from {occupied}"
        );
        for &addr in live {
            assert!(
                index.may_contain(addr),
                "a live address was dropped by a refresh"
            );
        }
        let still_admitted = retired.iter().filter(|&&a| index.may_contain(a)).count();
        assert!(
            still_admitted * 10 < retired.len(),
            "{still_admitted} of {} retired addresses still pass",
            retired.len()
        );
    }

    /// The defect the census measured is a WALK: bits accrue per admission, so a
    /// long session saturates even a filter sized for its live set. Churn a
    /// population two orders of magnitude larger than what is ever live and
    /// require that selectivity survives it.
    #[test]
    fn churn_does_not_walk_towards_saturation() {
        const CHURN: usize = 20_000;
        const RESIDENT: usize = 64;
        let addrs = wrapper_addrs(CHURN);
        let index: RegistryAddrIndex<256> = RegistryAddrIndex::new();
        let monotone: RegistryAddrFilter<256> = RegistryAddrFilter::new();
        for (i, &addr) in addrs.iter().enumerate() {
            index.admit(addr);
            monotone.admit(addr);
            if i >= RESIDENT {
                index.retire(addrs[i - RESIDENT]);
            }
        }
        assert!(index.live_count() <= RESIDENT + 1, "{}", index.live_count());
        let probes = probe_addrs(20_000);
        let rate = pass_rate(&index, &probes);
        assert!(
            rate < 0.02,
            "index admits {rate} of non-members after churn"
        );
        let monotone_passes = probes.iter().filter(|&&a| monotone.may_contain(a)).count();
        let monotone_rate = monotone_passes as f64 / probes.len() as f64;
        assert!(
            monotone_rate > 0.50,
            "the monotone witness of the same size should be saturated, got {monotone_rate}"
        );
        for i in (CHURN - RESIDENT)..CHURN {
            assert!(
                index.may_contain(addrs[i]),
                "churn dropped a resident address"
            );
        }
    }

    #[test]
    fn starts_idle_and_arms_once() {
        let latch = RegistryLatch::new();
        assert!(latch.is_idle());
        assert!(!latch.is_armed());
        latch.arm();
        assert!(!latch.is_idle());
        assert!(latch.is_armed());
        // Monotone: re-arming is a no-op, and there is deliberately no way back.
        latch.arm();
        assert!(latch.is_armed());
    }

    #[test]
    fn arm_is_visible_to_another_thread() {
        static LATCH: RegistryLatch = RegistryLatch::new();
        assert!(LATCH.is_idle());
        std::thread::spawn(|| LATCH.arm()).join().unwrap();
        assert!(LATCH.is_armed());
    }

    #[test]
    fn empty_window_contains_nothing() {
        let w = RegistryAddrWindow::new();
        assert_eq!(w.bounds_for_tests(), None);
        for addr in [0usize, 1, 0x1000, usize::MAX / 2, usize::MAX] {
            assert!(
                !w.may_contain(addr),
                "an empty window must reject {addr:#x} — it stands in for an idle latch"
            );
        }
    }

    #[test]
    fn window_only_ever_widens_and_never_rejects_an_admitted_address() {
        let w = RegistryAddrWindow::new();
        w.admit(0x3000);
        assert_eq!(w.bounds_for_tests(), Some((0x3000, 0x3000)));
        assert!(w.may_contain(0x3000));
        assert!(!w.may_contain(0x2fff));
        assert!(!w.may_contain(0x3001));

        w.admit(0x9000);
        assert_eq!(w.bounds_for_tests(), Some((0x3000, 0x9000)));
        // Both admitted addresses stay inside, and so does everything between.
        assert!(w.may_contain(0x3000));
        assert!(w.may_contain(0x6000));
        assert!(w.may_contain(0x9000));
        assert!(!w.may_contain(0x2fff));
        assert!(!w.may_contain(0x9001));

        // Re-admitting an interior address must not narrow anything.
        w.admit(0x6000);
        assert_eq!(w.bounds_for_tests(), Some((0x3000, 0x9000)));
    }

    /// Concurrent registration must not lose an address. With a load-then-store
    /// `admit` the two threads' stores race and the narrower bound can land
    /// last, evicting the other thread's live registration from the window —
    /// a false negative, which is a misclassification rather than a slowdown.
    /// `fetch_min`/`fetch_max` make that impossible, so this passes
    /// deterministically here and fails with high probability on the racy form.
    #[test]
    fn concurrent_admits_never_drop_an_address() {
        const THREADS: usize = 8;
        const PER_THREAD: usize = 512;
        static WINDOW: RegistryAddrWindow = RegistryAddrWindow::new();
        let handles: Vec<_> = (0..THREADS)
            .map(|t| {
                std::thread::spawn(move || {
                    // Interleave the ranges so every thread admits both very
                    // low and very high addresses, maximising the number of
                    // genuine bound updates that can race.
                    for i in 0..PER_THREAD {
                        WINDOW.admit(0x1_0000 + i * THREADS + t);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        for t in 0..THREADS {
            for i in 0..PER_THREAD {
                let addr = 0x1_0000 + i * THREADS + t;
                assert!(
                    WINDOW.may_contain(addr),
                    "{addr:#x} was admitted but the window lost it: {:?}",
                    WINDOW.bounds_for_tests()
                );
            }
        }
    }

    #[test]
    fn empty_filter_contains_nothing() {
        let f: RegistryAddrFilter = RegistryAddrFilter::new();
        assert_eq!(f.bits_set(), 0);
        for addr in [0usize, 8, 0x1000, 0x7fff_ffff_f000, usize::MAX] {
            assert!(
                !f.may_contain(addr),
                "an empty filter must contain nothing, but accepted {addr:#x}"
            );
        }
    }

    /// The one direction that must never fail: an admitted address is always
    /// accepted afterwards. A Bloom filter is allowed to accept addresses it
    /// was never given; it is never allowed to reject one it was.
    #[test]
    fn filter_never_rejects_an_admitted_address() {
        let f: RegistryAddrFilter = RegistryAddrFilter::new();
        let mut admitted = Vec::new();
        // A realistic spread: 8-byte-aligned addresses across a wide heap.
        for i in 0..256usize {
            let addr = 0x1_0000_0000usize + i * 0x2a8;
            f.admit(addr);
            admitted.push(addr);
            for &a in &admitted {
                assert!(
                    f.may_contain(a),
                    "{a:#x} was admitted and must stay accepted after {i} \
                     further admissions"
                );
            }
        }
        assert!(f.bits_set() > 0);
    }

    /// A filter that accepted everything would satisfy the test above without
    /// doing anything, so this is the half that makes it able to fail: with a
    /// realistic population the filter must still reject the overwhelming
    /// majority of addresses it was never given.
    #[test]
    fn filter_rejects_almost_everything_it_was_never_given() {
        let f: RegistryAddrFilter = RegistryAddrFilter::new();
        // 160 entries — the number of class prototypes `claude-code --help`
        // registers, i.e. the population this size was chosen for.
        for i in 0..160usize {
            f.admit(0x2_0000_0000usize + i * 0x330);
        }
        let probes = 20_000usize;
        let accepted = (0..probes)
            .filter(|i| f.may_contain(0x3_0000_0000usize + i * 8))
            .count();
        assert!(
            accepted * 20 < probes,
            "at 160 entries the filter must reject far more than 95% of \
             unregistered addresses; it accepted {accepted} of {probes}"
        );
    }

    #[test]
    fn concurrent_filter_admits_never_drop_an_address() {
        static FILTER: RegistryAddrFilter = RegistryAddrFilter::new();
        const PER_THREAD: usize = 512;
        let handles: Vec<_> = (0..4usize)
            .map(|t| {
                std::thread::spawn(move || {
                    for i in 0..PER_THREAD {
                        FILTER.admit(0x5_0000_0000usize + (t * PER_THREAD + i) * 8);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        for t in 0..4usize {
            for i in 0..PER_THREAD {
                let addr = 0x5_0000_0000usize + (t * PER_THREAD + i) * 8;
                assert!(
                    FILTER.may_contain(addr),
                    "{addr:#x} was admitted by thread {t} and must be visible \
                     here — a lost `fetch_or` is a misclassified pointer"
                );
            }
        }
    }

    #[test]
    fn filter_admit_is_visible_to_another_thread() {
        static FILTER: RegistryAddrFilter = RegistryAddrFilter::new();
        assert!(!FILTER.may_contain(0x9_0000_1000));
        std::thread::spawn(|| FILTER.admit(0x9_0000_1000))
            .join()
            .unwrap();
        assert!(FILTER.may_contain(0x9_0000_1000));
    }

    #[test]
    fn window_admit_is_visible_to_another_thread() {
        static WINDOW: RegistryAddrWindow = RegistryAddrWindow::new();
        assert!(!WINDOW.may_contain(0x4000));
        std::thread::spawn(|| WINDOW.admit(0x4000)).join().unwrap();
        assert!(WINDOW.may_contain(0x4000));
    }
}
