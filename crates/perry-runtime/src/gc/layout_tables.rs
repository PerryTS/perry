//! The **per-object** halves of the GC slot-layout metadata, and the emptiness
//! flag that keeps them off the hot path (#7510).
//!
//! Two address-keyed thread-locals live here:
//!
//! - [`LAYOUT_SLOT_MASKS`] — which slots of an object hold pointers.
//! - [`TYPED_LAYOUTS`] — the object's canonical `TypedLayoutDescriptor`.
//!
//! Both predate #6893, which moved the *common* case — an object whose live
//! layout still matches its shape — into the shape-keyed `SHAPE_LAYOUTS` map
//! in [`super::layout`]. What is left in these two maps is the residue:
//! objects that **diverged** from their shape, objects with no `keys_array`,
//! and ambiguous shapes. On a monomorphic workload that residue is empty for
//! the entire run.
//!
//! Empty is not the same as free, though. Every allocation
//! (`layout_init_pointer_free`), every typed-shape install, every object death
//! (`layout_clear_for_ptr`) and every relocation (`layout_transfer`) probed
//! both maps to clear whatever a previous tenant of a recycled address might
//! have left. Two `RefCell` round-trips plus two hashes, per object, to remove
//! nothing: `layout_forget_object` was 14.5% of self time on the
//! object-construction profile in #7510, nearly twice the allocator it was
//! bookkeeping for.
//!
//! [`PER_OBJECT_LAYOUTS_NONEMPTY`] answers "is there anything in either map at
//! all" in a single load, and every mutating path in this module maintains it.
//! Callers outside get the guarded accessors, not the maps.
//!
//! Split out of `layout.rs` to stay under the repo's 2000-line-per-file cap
//! (`scripts/check_file_size.sh`).

use super::hot_tls::{hot_layout_slot_masks, hot_per_object_layout_hint, hot_typed_layouts};
use super::layout::{LayoutSlotMask, TypedLayoutDescriptor};
use super::types::{GcHeader, GC_HEADER_SIZE, GC_TYPE_ARRAY, GC_TYPE_OBJECT};
use std::cell::{Cell, RefCell};

thread_local! {
    pub(in crate::gc) static LAYOUT_SLOT_MASKS: RefCell<crate::fast_hash::PtrHashMap<usize, LayoutSlotMask>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
    pub(in crate::gc) static TYPED_LAYOUTS: RefCell<crate::fast_hash::PtrHashMap<usize, TypedLayoutDescriptor>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
    /// #7510: "either per-object side table above **may** hold an entry".
    ///
    /// INVARIANT: `false` ⟹ both [`LAYOUT_SLOT_MASKS`] and [`TYPED_LAYOUTS`]
    /// are empty. Only an insert can break that emptiness, and every insert
    /// routes through [`typed_layouts_insert`] / [`slot_masks_insert`], which
    /// arm the flag; the removal paths re-test both maps and clear it again
    /// once they are empty. A stale `true` therefore costs exactly the
    /// pre-#7510 probe and nothing else — the flag is an accelerator, never an
    /// authority, and no caller may treat it as one.
    pub(in crate::gc) static PER_OBJECT_LAYOUTS_NONEMPTY: PerObjectLayoutHint =
        const { PerObjectLayoutHint::new() };
}

/// The flag, the address filter, and the filter's rebuild counter in ONE
/// thread-local.
///
/// They are co-located for a measured reason. On Darwin a thread-local access
/// is an out-of-line `_tlv_get_addr` call, and `crate::tls_hot` exists to pay
/// that once per hot region instead of once per table. `layout_forget_object`
/// consults the flag and then the filter on every allocation, death and
/// relocation; as two separate thread-locals that is two slot reads on exactly
/// the workloads that are legitimately armed, which measured +3.4% on `interp`
/// and +4.6% on `iso_miss`. One struct behind the existing named hot slot makes
/// it one.
pub(in crate::gc) struct PerObjectLayoutHint {
    /// #7510's global emptiness proof — see [`PER_OBJECT_LAYOUTS_NONEMPTY`].
    pub(in crate::gc) nonempty: Cell<bool>,
    /// Bits set in `filter` since the last rebuild.
    pub(in crate::gc) sets: Cell<u32>,
    /// Which addresses may have an entry — see [`layout_addr_filter_may_hold`].
    pub(in crate::gc) filter: std::cell::UnsafeCell<[u64; LAYOUT_ADDR_FILTER_WORDS]>,
}

impl PerObjectLayoutHint {
    const fn new() -> Self {
        Self {
            nonempty: Cell::new(false),
            sets: Cell::new(0),
            filter: std::cell::UnsafeCell::new([0u64; LAYOUT_ADDR_FILTER_WORDS]),
        }
    }
}

impl Drop for PerObjectLayoutHint {
    fn drop(&mut self) {
        // The ownership bit and its teardown live in this ONE TLS value. The
        // side-table keys may already have been destroyed (TLS destructor
        // order is deliberately irrelevant); `nonempty` is the authority for
        // whether this thread contributed to the process-global count.
        if self.nonempty.get() {
            per_object_layouts_global_disarm();
        }
    }
}

/// Bits in the per-object address filter (see [`layout_addr_filter_may_hold`]).
/// 4096 bits is 512 B of thread-local storage, held INLINE in
/// [`PerObjectLayoutHint`] so the flag and the filter share one hot slot. One
/// 8-byte word is read per query and the whole filter stays L1-resident even
/// when the addresses probed sweep a 16 MB nursery. Steady-state occupancy
/// after the `ImmortalLayoutScope` is one or two entries, so the false-positive
/// rate is ~0.05% — sizing this up buys nothing and costs inline TLS on every
/// thread.
const LAYOUT_ADDR_FILTER_BITS: usize = 4096;
const LAYOUT_ADDR_FILTER_WORDS: usize = LAYOUT_ADDR_FILTER_BITS / 64;

/// Process-global, monotone union of every thread's address filter.
///
/// `layout_forget_object` runs on every allocation once any thread holds a
/// per-object record, and its first real step used to be resolving this
/// thread's hint through `_tlv_get_addr` just to consult the filter. One
/// long-lived masked object (a test harness's callback closure, a registered
/// listener) therefore taxed every later allocation in the program with a
/// thread-local access — 3.4% of an allocation-heavy ECS row. Bits are set
/// alongside the thread-local filter and never cleared: a stale bit is only a
/// false positive that falls through to the thread-local check, whereas
/// clearing while another thread still holds records would be a false
/// negative and leave a stale mask on a recycled address. The filter is a
/// 4,096-bit sketch, so saturation degrades to exactly the previous cost.
static GLOBAL_LAYOUT_ADDR_FILTER: [std::sync::atomic::AtomicU64; LAYOUT_ADDR_FILTER_WORDS] =
    [const { std::sync::atomic::AtomicU64::new(0) }; LAYOUT_ADDR_FILTER_WORDS];

#[inline(always)]
pub(in crate::gc) fn global_layout_addr_filter_may_hold(user_ptr: usize) -> bool {
    let (word, bit) = layout_addr_filter_slot(user_ptr);
    GLOBAL_LAYOUT_ADDR_FILTER[word].load(std::sync::atomic::Ordering::Relaxed) & bit != 0
}
/// Rebuild the filter from the live keys once this many bits have been set
/// since the last rebuild. Without it a workload that churns per-object
/// records would saturate the filter and never recover; with it the false
/// positive rate is bounded by (live entries / bits) rather than by
/// (entries ever inserted / bits), at an amortised O(1) per insert.
const LAYOUT_ADDR_FILTER_REBUILD_AFTER: u32 = (LAYOUT_ADDR_FILTER_BITS / 2) as u32;

/// Word index + bit mask for `user_ptr`. Heap pointers are at least 8-byte
/// aligned and clustered, so the low bits alone would collide systematically;
/// a single multiply spreads the whole address across the filter.
#[inline(always)]
fn layout_addr_filter_slot(user_ptr: usize) -> (usize, u64) {
    let h = (user_ptr as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let idx = (h >> (64 - LAYOUT_ADDR_FILTER_BITS.trailing_zeros() as u64)) as usize;
    (idx >> 6, 1u64 << (idx & 63))
}

/// Could either per-object side table hold an entry keyed by `user_ptr`?
///
/// `false` is a **proof of absence**; `true` is a hint (a real entry, or a
/// collision). This is the address-precise half of the accelerator, and it is
/// the half that survives an immortal resident: [`PER_OBJECT_LAYOUTS_NONEMPTY`]
/// is a single global bit, so ONE entry that is never removed — one long-lived
/// object anywhere in the process — turns `layout_forget_object` back into two
/// `RefCell` round-trips plus two hashes on every allocation, death and
/// relocation for the rest of the run. Removing 1113 of 1115 such entries buys
/// nothing while the last two remain; only an address-keyed test does.
///
/// ## Why the flag is still tested FIRST, even though this subsumes it
///
/// It does subsume it — both maps empty implies every bit is clear, because the
/// flag's own clear path clears the filter — and dropping the flag would make
/// the armed path one thread-local resolution cheaper. Measured on the quiet
/// mini, that trade is a loss: filter-only moved `interp` 1.948 → 1.934 and
/// `iso_miss` 2.476 → 2.464, but moved `push_cls` 0.367 → 0.383 (past its
/// budget), `churn` 0.422 → 0.438 and `tree` 1.642 → 1.673. Almost every
/// workload is *disarmed*, and for those the flag is a single load while this
/// is a multiply, a shift, a load and a test. The flag stays in front as the
/// cheap common-case gate; the filter is what rescues the armed case.
///
/// The residual cost of consulting both — a second thread-local resolution on a
/// legitimately-armed workload, +3.4% on `interp` and +4.6% on `iso_miss` —
/// is not inherent. It goes away by co-locating the flag and this filter in one
/// thread-local so the armed path resolves once; that is a `tls_hot` change and
/// is deliberately left out of this one.
#[inline(always)]
pub(in crate::gc) fn layout_addr_filter_may_hold(user_ptr: usize) -> bool {
    hint_may_hold(hot_per_object_layout_hint(), user_ptr)
}

/// [`layout_addr_filter_may_hold`] against an already-resolved hint, so a
/// caller that also reads the flag pays ONE slot resolution for both.
#[inline(always)]
pub(in crate::gc) fn hint_may_hold(hint: &PerObjectLayoutHint, user_ptr: usize) -> bool {
    let (word, bit) = layout_addr_filter_slot(user_ptr);
    unsafe { (*hint.filter.get())[word] & bit != 0 }
}

/// Record that `user_ptr` now has an entry. Called by every insert site.
/// The rebuild check runs BEFORE the bit is set, never after: a rebuild
/// reconstructs the filter from the maps' live keys, and this key is not in
/// the map yet at any call site, so rebuilding afterwards would erase the bit
/// just set and make a live record invisible to the filter.
#[inline]
fn layout_addr_filter_add(user_ptr: usize) {
    if hot_per_object_layout_hint().sets.get() >= LAYOUT_ADDR_FILTER_REBUILD_AFTER {
        layout_addr_filter_rebuild();
    }
    layout_addr_filter_note(user_ptr);
}

/// Set `user_ptr`'s bit WITHOUT the rebuild check.
///
/// `layout_note_slot` calls this from inside its own `borrow_mut` on
/// `LAYOUT_SLOT_MASKS`, and [`layout_addr_filter_rebuild`] borrows both maps —
/// so the rebuilding form would panic there. Skipping the rebuild is safe: the
/// counter is an accuracy heuristic, not a correctness one.
#[inline]
pub(in crate::gc) fn layout_addr_filter_note(user_ptr: usize) {
    let hint = hot_per_object_layout_hint();
    let (word, bit) = layout_addr_filter_slot(user_ptr);
    unsafe {
        (*hint.filter.get())[word] |= bit;
    }
    GLOBAL_LAYOUT_ADDR_FILTER[word].fetch_or(bit, std::sync::atomic::Ordering::Relaxed);
    hint.sets.set(hint.sets.get().saturating_add(1));
}

/// Drop every bit and re-add the live keys. Removals cannot clear a bit on
/// their own (two keys may share one), so this is what keeps a workload that
/// genuinely churns per-object records from saturating the filter forever.
fn layout_addr_filter_rebuild() {
    layout_addr_filter_clear();
    let keys: Vec<usize> = {
        let masks = hot_layout_slot_masks().borrow();
        let typed = hot_typed_layouts().borrow();
        masks.keys().copied().chain(typed.keys().copied()).collect()
    };
    let hint = hot_per_object_layout_hint();
    for k in keys {
        let (word, bit) = layout_addr_filter_slot(k);
        unsafe {
            (*hint.filter.get())[word] |= bit;
        }
    }
    hint.sets.set(0);
}

fn layout_addr_filter_clear() {
    let hint = hot_per_object_layout_hint();
    unsafe {
        (*hint.filter.get()).fill(0);
    }
    hint.sets.set(0);
}

crate::perry_thread_local! {
    /// Nesting depth of the innermost [`ImmortalLayoutScope`].
    ///
    /// Read only from the *cold* half of `layout_note_slot` — the branch that
    /// would otherwise mint a brand-new per-object mask — so an inactive scope
    /// costs nothing on any hot path.
    static IMMORTAL_LAYOUT_SCOPE_DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// True while an [`ImmortalLayoutScope`] is open on this thread.
#[inline]
pub(in crate::gc) fn immortal_layout_scope_active() -> bool {
    IMMORTAL_LAYOUT_SCOPE_DEPTH.with(|d| d.get()) != 0
}

/// Marks a window whose objects are **immortal by construction** — reachable
/// from a GC root for the life of the process — so that none of them may take
/// out a per-object layout record.
///
/// ## Why this exists (#7510's lesson, repeating)
///
/// [`PER_OBJECT_LAYOUTS_NONEMPTY`] is a *global emptiness* proof: it turns
/// `layout_forget_object` — which runs on every allocation, every object death
/// and every relocation — into a single load whenever both side tables happen
/// to be empty. On a monomorphic workload they are empty for the entire run,
/// which is exactly what makes the accelerator worth having.
///
/// A *single* entry that is never removed converts that accelerator into
/// permanently-disabled code. #7510 already paid this once, via one interned
/// keys-array the shape cache anchored forever. The `globalThis` bootstrap is
/// the same trap at several hundred times the scale: it builds hundreds of
/// plain objects whose first pointer field mints a mask, every one of them
/// rooted at `globalThis` forever. Since a plain-object property miss forces
/// that bootstrap, *every real TypeScript program* armed the regime before
/// user code ran — measured as +28% on `churn` and +29% on `tree`, with
/// `layout_forget_object` going 112 → 916 ms and 194 → 740 ms of self time.
///
/// ## Why dropping the mask is sound
///
/// The alternative the code already uses for the very same situation — a
/// pointer stored into an object whose state is not `POINTER_FREE` — is
/// [`super::layout::GC_LAYOUT_UNKNOWN`], the tag-checked payload scan. That is
/// the universally safe state, not a weaker one; the mask is a *precision*
/// optimization that lets the collector skip known-pointer-free slots.
///
/// For an immortal object that precision buys nothing measurable: the object
/// is never reclaimed, and it is scanned only as part of the root graph. It
/// costs one tag test per slot on a few hundred objects, against two `RefCell`
/// round-trips and two hash probes on every allocation the program will ever
/// make.
///
/// ## Deliberately NOT applied to typed-shape layouts
///
/// The scope gates ONLY the mask-minting branch of `layout_note_slot`. It must
/// never redirect a *typed* layout (`init_typed_shape_layout`,
/// `layout_rebuild_from_slots`) to `GC_LAYOUT_UNKNOWN`: those describe objects
/// with raw-f64 slots, and a raw double's bit pattern can alias a heap pointer.
/// A conservative scan would then trace — and, under a copying collector,
/// *rewrite* — a slot holding a number. `GC_LAYOUT_UNKNOWN` is only safe where
/// every slot is a NaN-boxed value, which is what the mask-minting branch
/// already assumes.
pub struct ImmortalLayoutScope {
    _not_send: std::marker::PhantomData<*const ()>,
}

impl Default for ImmortalLayoutScope {
    fn default() -> Self {
        Self::new()
    }
}

impl ImmortalLayoutScope {
    pub fn new() -> Self {
        IMMORTAL_LAYOUT_SCOPE_DEPTH.with(|d| d.set(d.get().saturating_add(1)));
        Self {
            _not_send: std::marker::PhantomData,
        }
    }
}

impl Drop for ImmortalLayoutScope {
    fn drop(&mut self) {
        IMMORTAL_LAYOUT_SCOPE_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

/// Live entry counts of the two per-object side tables, for `PERRY_GC_DIAG`
/// and for the tests that assert the bootstrap left them alone.
pub(crate) fn per_object_layout_table_sizes() -> (usize, usize) {
    (
        hot_layout_slot_masks().borrow().len(),
        hot_typed_layouts().borrow().len(),
    )
}

/// Smallest payload slot count for which minting a **per-object pointer mask**
/// is worth its side-table entry. Below it the object takes
/// `GC_LAYOUT_UNKNOWN` — the tag-checked scan-all-slots state — instead.
///
/// The two sides are not symmetric. A mask's benefit is bounded by the object:
/// it can skip at most `slots - pointers` tag checks per trace. Its cost is
/// **program-global and unbounded** — one live entry arms
/// [`PER_OBJECT_LAYOUTS_NONEMPTY`], which puts a two-map hash probe back on
/// every allocation anywhere in the program for as long as that entry lives
/// (see the module docs, and #7510's "one immortal entry nullifies
/// `is_empty()`"). At the bottom of the range the asymmetry is total rather
/// than merely lopsided: over a **single** slot a mask cannot skip anything at
/// all, because the tracer consults `layout_pointer_bearing_bits` on that one
/// slot either way, so the entry is the mask's entire contribution. At two or
/// three slots it can skip only one or two such checks.
///
/// A tag check is exact at both mint sites: neither is reached for an object
/// with an intact typed descriptor, so there are no raw-f64 slots whose bits a
/// tag check could misread as a pointer. #7630 recorded the same conclusion for
/// the materialiser cohort — "a pointer mask can never skip anything a tag
/// check would not reject anyway ... the mask machinery buys nothing here".
///
/// `PERRY_LAYOUT_MASK_MIN_SLOTS` overrides it for bisection.
#[inline(always)]
pub(in crate::gc) fn layout_mask_min_slots() -> usize {
    use std::sync::atomic::{AtomicUsize, Ordering};
    /// `usize::MAX` = "not yet read from the environment".
    static N: AtomicUsize = AtomicUsize::new(usize::MAX);
    match N.load(Ordering::Relaxed) {
        usize::MAX => {
            let v = std::env::var("PERRY_LAYOUT_MASK_MIN_SLOTS")
                .ok()
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(DEFAULT_MASK_MIN_SLOTS);
            N.store(v, Ordering::Relaxed);
            v
        }
        v => v,
    }
}

/// Payloads with up to three slots take the scan. At that size the mask can
/// skip at most two exact tag checks, while one long-lived side-table entry
/// arms address cleanup on every allocation in the program.
///
/// Measured on the 19-benchmark corpus (M1 mini, five shuffled interleaved
/// repeats against the same binaries, medians):
///
/// | bench | threshold 2 instructions | threshold 4 instructions | delta |
/// |---|--:|--:|--:|
/// | `interp` | 9,013,545,066 | 8,598,251,738 | **-4.61%** |
/// | `iso_miss` | 12,494,387,198 | 12,489,410,538 | -0.04% |
///
/// Every other benchmark moved by less than 0.37%; `interp` peak RSS was
/// byte-identical at 32,964,608 in both arms. Wall ranges overlapped on the
/// contended host, so retired instructions are the primary signal.
///
/// A current threshold sweep found the full `interp` win at `4`; higher values
/// did not retire fewer instructions. Keeping the smallest winning threshold
/// bounds the extra trace work and changes the fewest layout preconditions.
pub(in crate::gc) const DEFAULT_MASK_MIN_SLOTS: usize = 4;

/// True when either per-object side table may hold an entry. `false` is a
/// proof of emptiness (see [`PER_OBJECT_LAYOUTS_NONEMPTY`]); `true` is only a
/// hint, so every caller still has to handle a miss.
#[inline(always)]
pub(in crate::gc) fn per_object_layouts_maybe_nonempty() -> bool {
    hot_per_object_layout_hint().nonempty.get()
}

/// Arm the flag. Called by anything that inserts into either map — including
/// the one insert site that holds its own `borrow_mut` and so cannot go
/// through the wrappers below.
#[inline(always)]
pub(in crate::gc) fn mark_per_object_layouts_nonempty() {
    let hint = hot_per_object_layout_hint();
    if !hint.nonempty.replace(true) {
        per_object_layouts_global_arm();
    }
}

/// Process-global count of threads whose [`PER_OBJECT_LAYOUTS_NONEMPTY`] is
/// armed, exported so **generated code** can test it with one load (#7834).
///
/// `layout_forget_object` is a runtime call on every inline-bump construction,
/// and on a monomorphic workload every one of those calls returns immediately
/// having proved emptiness. The proof itself is thread-local, so codegen could
/// not read it: a `_tlv_get_addr` from generated code costs more than the call
/// it would replace. This count is the same proof in a plain `static`, so the
/// construction site becomes `load atomic i32` + a never-taken branch.
///
/// `0` is a proof that **no thread** holds a per-object layout record, and so
/// that no recycled address can carry a stale one. A non-zero count is only a
/// hint — the call it gates re-tests the thread-local flag and the address
/// filter, exactly as it always did.
///
/// This exported count is the ONE authoritative state. Keeping a separate
/// count and byte permits a disarm/re-arm interleaving to publish a false zero
/// after the re-arm (#7873), regardless of memory ordering. The owning TLS
/// value's destructor also removes its contribution when a worker exits with
/// records still live.
#[no_mangle]
pub static PERRY_PER_OBJECT_LAYOUTS_ANY: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);

#[inline(never)]
fn per_object_layouts_global_arm() {
    use std::sync::atomic::Ordering;
    PERRY_PER_OBJECT_LAYOUTS_ANY
        .try_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
            count.checked_add(1)
        })
        .expect("per-object layout thread count overflow");
}

#[inline(never)]
fn per_object_layouts_global_disarm() {
    per_object_layouts_global_disarm_with_hook(&PERRY_PER_OBJECT_LAYOUTS_ANY, || {});
}

fn per_object_layouts_global_disarm_with_hook(
    armed_threads: &std::sync::atomic::AtomicU32,
    after_decrement: impl FnOnce(),
) {
    use std::sync::atomic::Ordering;
    armed_threads
        .try_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
            count.checked_sub(1)
        })
        .expect("per-object layout thread count underflow");
    // Test hook for #7873's former vulnerable window. There is no second
    // publication after this point: a concurrent re-arm updates this same
    // atomic, so it cannot be overwritten by a delayed zero store.
    after_decrement();
}

#[cfg(test)]
pub(in crate::gc) fn test_per_object_layouts_global_disarm_with_hook(
    armed_threads: &std::sync::atomic::AtomicU32,
    after_decrement: impl FnOnce(),
) {
    per_object_layouts_global_disarm_with_hook(armed_threads, after_decrement);
}

#[cfg(test)]
pub(in crate::gc) fn test_per_object_layout_armed_threads() -> u32 {
    PERRY_PER_OBJECT_LAYOUTS_ANY.load(std::sync::atomic::Ordering::SeqCst)
}

/// The generated-code entry point for [`layout_forget_object`] (#7834).
///
/// An inline-bump `new` site that baked its layout state into the header
/// constant still has to clear whatever a previous tenant of the recycled
/// address left in the per-object tables — that is the one part of
/// `js_gc_declare_typed_shape_layout` which depends on the address rather than
/// on the shape. Codegen emits this call behind a
/// [`PERRY_PER_OBJECT_LAYOUTS_ANY`] test, so it runs only in the armed regime.
#[no_mangle]
pub extern "C" fn js_gc_forget_object_layout(obj: u64) {
    let user_ptr = super::layout::strip_nanbox_user_ptr(obj);
    if user_ptr == 0 {
        return;
    }
    layout_forget_object(user_ptr);
}

/// Re-establish the flag after a removal emptied one map: clear it once the
/// *other* one is empty too.
///
/// Callers pass the emptiness of the map they just touched, so "removed one of
/// many" never takes a second borrow. A workload that genuinely keeps
/// per-object records (`tree`) removes far more often than it empties, and
/// must not pay for a fast path it is not getting.
#[inline]
pub(in crate::gc) fn refresh_per_object_layouts_flag(touched_map_emptied: bool) {
    if !touched_map_emptied {
        return;
    }
    if hot_layout_slot_masks().borrow().is_empty() && hot_typed_layouts().borrow().is_empty() {
        if hot_per_object_layout_hint().nonempty.replace(false) {
            per_object_layouts_global_disarm();
        }
        // Both maps are empty, so every bit is now stale. Clearing here is what
        // makes the filter's occupancy track LIVE entries rather than every
        // entry the program has ever created.
        layout_addr_filter_clear();
    }
}

/// The one way to add a per-object typed descriptor.
#[inline]
pub(in crate::gc) fn typed_layouts_insert(user_ptr: usize, descriptor: TypedLayoutDescriptor) {
    mark_per_object_layouts_nonempty();
    layout_addr_filter_add(user_ptr);
    hot_typed_layouts()
        .borrow_mut()
        .insert(user_ptr, descriptor);
}

/// The one way to add a per-object pointer mask.
#[inline]
pub(in crate::gc) fn slot_masks_insert(user_ptr: usize, mask: LayoutSlotMask) {
    mark_per_object_layouts_nonempty();
    layout_addr_filter_add(user_ptr);
    hot_layout_slot_masks().borrow_mut().insert(user_ptr, mask);
}

/// Drop `user_ptr`'s per-object typed descriptor (only).
#[inline]
pub(in crate::gc) fn typed_layouts_remove(user_ptr: usize) {
    if !per_object_layouts_maybe_nonempty() || !layout_addr_filter_may_hold(user_ptr) {
        return;
    }
    let emptied = {
        let mut typed = hot_typed_layouts().borrow_mut();
        typed.remove(&user_ptr).is_some() && typed.is_empty()
    };
    refresh_per_object_layouts_flag(emptied);
}

/// Drop `user_ptr`'s per-object pointer mask (only).
#[inline]
pub(in crate::gc) fn slot_masks_remove(user_ptr: usize) {
    if !per_object_layouts_maybe_nonempty() || !layout_addr_filter_may_hold(user_ptr) {
        return;
    }
    let emptied = {
        let mut masks = hot_layout_slot_masks().borrow_mut();
        masks.remove(&user_ptr).is_some() && masks.is_empty()
    };
    refresh_per_object_layouts_flag(emptied);
}

/// Run `f` against `user_ptr`'s per-object typed descriptor, if it has one.
/// The borrow is confined to `f` because the callers that act on the answer
/// (`layout_set_typed_unknown`) take the same map mutably.
#[inline]
pub(in crate::gc) fn with_per_object_descriptor<R>(
    user_ptr: usize,
    f: impl FnOnce(&TypedLayoutDescriptor) -> R,
) -> Option<R> {
    if !per_object_layouts_maybe_nonempty() || !layout_addr_filter_may_hold(user_ptr) {
        return None;
    }
    hot_typed_layouts().borrow().get(&user_ptr).map(f)
}

/// `user_ptr`'s per-object pointer mask, if it has one. The trace path calls
/// this once per `SIDE_MASK` object it visits, so the emptiness proof is worth
/// as much here as it is on the mutator side.
#[inline]
pub(in crate::gc) fn per_object_slot_mask(user_ptr: usize) -> Option<LayoutSlotMask> {
    if !per_object_layouts_maybe_nonempty() || !layout_addr_filter_may_hold(user_ptr) {
        return None;
    }
    hot_layout_slot_masks().borrow().get(&user_ptr).cloned()
}

/// Move `old_user`'s per-object typed descriptor to `new_user` (relocation),
/// clearing anything the destination address inherited from a previous tenant.
/// Returns whether a descriptor actually made the move — `layout_transfer`
/// uses that to decide the destination's intact bit.
///
/// With both maps provably empty there is nothing to move, and every relocated
/// object would otherwise pay a `RefCell` round-trip plus two hashes during
/// evacuation. The shape-keyed half is unaffected: it needs no move at all.
#[inline]
pub(in crate::gc) fn transfer_per_object_descriptor(old_user: usize, new_user: usize) -> bool {
    // BOTH addresses are touched (the destination is cleared of a previous
    // tenant's record before the source's is moved in), so the filter can only
    // prove this call unnecessary when it proves both absent.
    if !per_object_layouts_maybe_nonempty()
        || (!layout_addr_filter_may_hold(old_user) && !layout_addr_filter_may_hold(new_user))
    {
        return false;
    }
    let mut typed = hot_typed_layouts().borrow_mut();
    typed.remove(&new_user);
    match typed.remove(&old_user) {
        Some(layout) => {
            typed.insert(new_user, layout);
            drop(typed);
            layout_addr_filter_add(new_user);
            true
        }
        None => false,
    }
}

/// Move `old_user`'s per-object pointer mask to `new_user` (relocation).
#[inline]
pub(in crate::gc) fn transfer_per_object_slot_mask(old_user: usize, new_user: usize) {
    if !per_object_layouts_maybe_nonempty()
        || (!layout_addr_filter_may_hold(old_user) && !layout_addr_filter_may_hold(new_user))
    {
        return;
    }
    let mut masks = hot_layout_slot_masks().borrow_mut();
    masks.remove(&new_user);
    if let Some(mask) = masks.remove(&old_user) {
        masks.insert(new_user, mask);
        drop(masks);
        layout_addr_filter_add(new_user);
    }
}

/// Drop any per-object layout record keyed by `user_ptr`.
///
/// Both maps are probed on **every** object allocation
/// (`layout_init_pointer_free`), on every typed-shape install, and again on
/// object death, to clear whatever a previous tenant of a recycled address
/// left behind. Since #6893 there is usually nothing to clear, so the whole
/// call is pure cost — see the module docs.
///
/// [`PER_OBJECT_LAYOUTS_NONEMPTY`]'s `false` state is a proof of emptiness, so
/// the early return cannot skip a live record. The slow half below is the
/// pre-#7510 path unchanged, and re-arms the flag on the way out.
#[inline]
pub(in crate::gc) fn layout_forget_object(user_ptr: usize) {
    // #7834/#7873: the process-global atomic first, because reading it avoids
    // resolving `hot_per_object_layout_hint()` — on Darwin a thread-local
    // access is an out-of-line `_tlv_get_addr` call.
    // `0` proves every thread's tables are empty, which is the steady state of
    // every monomorphic workload, so the disarmed path now costs one load and
    // one branch instead of a call. (Measured as 6% of `cycles`, whose
    // pointer-bearing shape keeps the full runtime declare.)
    if PERRY_PER_OBJECT_LAYOUTS_ANY.load(std::sync::atomic::Ordering::SeqCst) == 0 {
        return;
    }
    // Process-global sketch before any thread-local access: an address no
    // thread ever recorded needs nothing removed.
    if !global_layout_addr_filter_may_hold(user_ptr) {
        return;
    }
    // ONE hot-slot resolution for both halves of the guard: the flag (cheap,
    // and false for the overwhelming majority of workloads) and then the
    // address filter (what rescues a workload with an immortal record).
    let hint = hot_per_object_layout_hint();
    if !hint.nonempty.get() || !hint_may_hold(hint, user_ptr) {
        return;
    }
    // One `borrow_mut` per map, not a `borrow` to test emptiness followed by a
    // second `borrow_mut` to remove: `RefCell`'s flag traffic is a measurable
    // share of a function this hot (#7469).
    //
    // The `is_some()` on the removal is what keeps the armed regime — a
    // workload like `tree` that genuinely holds per-object records — at the
    // pre-#7510 instruction count. Almost every call there is a fresh address
    // with nothing to remove, and a miss cannot have emptied anything, so the
    // trailing `is_empty()` never runs.
    let masks_emptied = {
        let mut masks = hot_layout_slot_masks().borrow_mut();
        !masks.is_empty() && masks.remove(&user_ptr).is_some() && masks.is_empty()
    };
    let typed_emptied = {
        let mut typed = hot_typed_layouts().borrow_mut();
        !typed.is_empty() && typed.remove(&user_ptr).is_some() && typed.is_empty()
    };
    refresh_per_object_layouts_flag(masks_emptied || typed_emptied);
}

#[cfg(test)]
pub(in crate::gc) fn test_per_object_tables_are_empty() -> bool {
    hot_layout_slot_masks().borrow().is_empty() && hot_typed_layouts().borrow().is_empty()
}

/// An upper bound on the payload slots the tracer would enumerate for
/// `user_ptr`, or `usize::MAX` when this module cannot cheaply tell.
///
/// Both directions of error are *correct*, only differently priced, which is
/// what lets this be a bound rather than an exact count: over-estimating mints
/// a mask that was not needed (the pre-existing behaviour), and
/// under-estimating routes the object to `GC_LAYOUT_UNKNOWN`, where the tracer
/// scans every slot and so visits a superset of what a mask would have
/// selected. Neither can hide a live child.
///
/// An array reports its `length` — exactly the range the tracer walks, and so
/// exactly the bound on what a mask could skip — but **only for a store into an
/// already-formed array**. A store at the append position (`slot_index >=
/// length`) reports `usize::MAX` instead, because every append protocol writes
/// the element and notes the slot *before* bumping `length` (see
/// [`layout_all_pointer_array_append`]): mid-construction `length` is the
/// pre-append value, usually 0 or 1, and judging on it would strand every
/// incrementally built array — a `push` loop, a JSON parse — in the scan state
/// no matter how large it eventually grew. Capacity is not a substitute:
/// `MIN_ARRAY_CAPACITY` is 16, so a one-element literal reports 16 and the
/// distinction this is drawing disappears.
///
/// An object reports the bound derived from [`GcHeader::size`] rather than its
/// `field_count`: `size` is maintained for every GC allocation whatever its
/// type-specific header says, so this stays correct for a payload that is not a
/// well-formed `ObjectHeader`, and it errs high — towards the old mask path.
#[inline]
pub(in crate::gc) unsafe fn layout_payload_slot_count(
    header: *const GcHeader,
    user_ptr: usize,
    slot_index: usize,
) -> usize {
    match (*header).obj_type {
        GC_TYPE_ARRAY => {
            let arr = user_ptr as *const crate::array::ArrayHeader;
            let length = (*arr).length as usize;
            let capacity = (*arr).capacity as usize;
            if length > capacity || length > 16_000_000 || slot_index >= length {
                usize::MAX
            } else {
                length
            }
        }
        GC_TYPE_OBJECT => {
            let size = (*header).size as usize;
            match size.checked_sub(GC_HEADER_SIZE) {
                Some(payload) => payload / 8,
                None => usize::MAX,
            }
        }
        _ => usize::MAX,
    }
}

/// True when `user_ptr` is small enough that a tag-checked scan of every slot
/// beats a per-object pointer mask. See [`layout_mask_min_slots`].
#[inline]
pub(in crate::gc) unsafe fn layout_prefers_scan_over_mask(
    header: *const GcHeader,
    user_ptr: usize,
    slot_index: usize,
) -> bool {
    layout_payload_slot_count(header, user_ptr, slot_index) < layout_mask_min_slots()
}
