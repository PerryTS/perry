//! Box runtime for mutable captured variables
//!
//! When a closure captures a variable that is modified (either in the closure
//! or in the outer scope), we need to store it in a heap-allocated "box" so
//! both scopes share the same storage location.

use std::alloc::{alloc, Layout};
use std::sync::atomic::{AtomicU64, Ordering};

static BOX_GET_NULL_COUNT: AtomicU64 = AtomicU64::new(0);
static BOX_SET_NULL_COUNT: AtomicU64 = AtomicU64::new(0);
static I32_BOX_GET_NULL_COUNT: AtomicU64 = AtomicU64::new(0);
static I32_BOX_SET_NULL_COUNT: AtomicU64 = AtomicU64::new(0);
static BOOL_BOX_GET_NULL_COUNT: AtomicU64 = AtomicU64::new(0);
static BOOL_BOX_SET_NULL_COUNT: AtomicU64 = AtomicU64::new(0);

// #7933 follow-up (async-state RSS accumulation) — release/reuse telemetry.
// `allocs` counts every `js_*box_alloc*` call, `pool_reuses` the subset served
// from the free pool instead of `std::alloc`, `releases` the cells parked by
// `js_*box_release`. `allocs - pool_reuses` is the number of cells ever
// malloc'd — the process-lifetime malloc residue the regression test gates on.
static BOX_ALLOC_COUNT: AtomicU64 = AtomicU64::new(0);
static BOX_POOL_REUSE_COUNT: AtomicU64 = AtomicU64::new(0);
static BOX_RELEASE_COUNT: AtomicU64 = AtomicU64::new(0);
// Diagnostic-only (#8208 tuning): how often the quarantine actually drains,
// and how many cells that published. A pool whose high-water mark is 41x one
// batch's working set is a flush-frequency question, not a pool-size question.
static BOX_FLUSH_COUNT: AtomicU64 = AtomicU64::new(0);
static BOX_FLUSH_PUBLISHED: AtomicU64 = AtomicU64::new(0);

/// Snapshot of the release/reuse counters: `(allocs, pool_reuses, releases)`.
/// Sums all three box kinds.
pub fn box_release_stats() -> (u64, u64, u64) {
    (
        BOX_ALLOC_COUNT.load(Ordering::Relaxed),
        BOX_POOL_REUSE_COUNT.load(Ordering::Relaxed),
        BOX_RELEASE_COUNT.load(Ordering::Relaxed),
    )
}

/// `PERRY_GC_DIAG=1`: one line at process exit with the box release/reuse
/// counters and the surviving registry populations. `resident = allocs -
/// pool_reuses` is the number of cells that cost a real `std::alloc`
/// allocation over the process lifetime — the quantity that grew linearly
/// with completed async activations before the #7933 follow-up. Emitted from
/// the same exit funnel as the other GC diagnostics
/// (`js_gc_release_current_thread_collection_side_allocations`).
pub fn report_box_stats_at_exit() {
    if !crate::gc::gc_diag_enabled() {
        return;
    }
    static EMITTED: AtomicU64 = AtomicU64::new(0);
    if EMITTED.swap(1, Ordering::SeqCst) != 0 {
        return;
    }
    let (allocs, reuses, releases) = box_release_stats();
    let (reg, i32_reg, bool_reg) = (
        BOX_REGISTRY.with(|r| r.borrow().len()),
        I32_BOX_REGISTRY.with(|r| r.borrow().len()),
        BOOL_BOX_REGISTRY.with(|r| r.borrow().len()),
    );
    eprintln!(
        "[box-stats] allocs={allocs} pool_reuses={reuses} releases={releases} \
         resident_cells={} registry_len={reg} i32_registry_len={i32_reg} \
         bool_registry_len={bool_reg} flushes={} published={}",
        allocs.saturating_sub(reuses),
        BOX_FLUSH_COUNT.load(Ordering::Relaxed),
        BOX_FLUSH_PUBLISHED.load(Ordering::Relaxed),
    );
}

/// A box is simply a heap-allocated JSValue bit slot.
#[repr(C)]
pub struct Box {
    pub value: u64,
}

#[repr(C, align(8))]
pub struct I32Box {
    pub value: i32,
}

#[repr(C, align(8))]
pub struct BoolBox {
    pub value: bool,
}

crate::perry_thread_local! {
    /// Registry of every active box pointer. GC traces the contained
    /// JSValue bits so that NaN-boxed heap pointers stored in boxes (e.g.
    /// the generator state machine's iter object held in `__iter`'s
    /// mutable-capture box) keep the referenced heap object alive
    /// across collections. Without this, captures stored as raw box
    /// pointers in closure capture slots fail the `valid_ptrs.contains`
    /// check during `trace_closure` (boxes come from `std::alloc::alloc`
    /// directly, not the GC arena), so the box pointer is never marked
    /// AND the JSValue bits inside are never scanned — heap objects
    /// referenced only through box-captures can be swept mid-await.
    pub(crate) static BOX_REGISTRY: std::cell::RefCell<crate::fast_hash::PtrHashSet<usize>> =
        // Pre-size for promise-heavy workloads: `promise_all_chains`
        // allocates ~150 k boxes per kernel run (one per closure
        // mutable capture). Starting at 128 k buckets (~2 MB) covers
        // the full working set in one alloc — without it, hashbrown
        // rehashes from 0 → 256 k buckets across the alloc history,
        // showing up as ~3 % CPU in `hash_one` / `reserve_rehash`.
        std::cell::RefCell::new(std::collections::HashSet::with_capacity_and_hasher(
            128 * 1024,
            crate::fast_hash::PtrHasher,
        ));
    pub(crate) static I32_BOX_REGISTRY: std::cell::RefCell<crate::fast_hash::PtrHashSet<usize>> =
        std::cell::RefCell::new(std::collections::HashSet::with_capacity_and_hasher(
            16 * 1024,
            crate::fast_hash::PtrHasher,
        ));
    pub(crate) static BOOL_BOX_REGISTRY: std::cell::RefCell<crate::fast_hash::PtrHashSet<usize>> =
        std::cell::RefCell::new(std::collections::HashSet::with_capacity_and_hasher(
            16 * 1024,
            crate::fast_hash::PtrHasher,
        ));
}

/// Number of slots in each registry's direct-mapped positive cache. Eight
/// covers the working set that matters: the async-to-generator state machine
/// re-reads the same handful of boxes (`__gen_state`, `__gen_done`,
/// `__gen_executing`, plus the activation's body locals) on every step, and
/// activations run one at a time.
const BOX_PTR_CACHE_SLOTS: usize = 8;

type BoxPtrCache = crate::tls_hot::HotKey<[std::cell::Cell<usize>; BOX_PTR_CACHE_SLOTS]>;

crate::perry_thread_local! {
    /// Direct-mapped **positive** cache over `BOX_REGISTRY`.
    ///
    /// `js_box_get`/`js_box_set` validate their operand against the registry on
    /// every access (perry#4898), and that hash probe is the single largest leaf
    /// in Perry's async machinery — the transform boxes every body local of an
    /// `async` function, so a state machine pays one probe per local read and
    /// one per write. Measured on a promise-only kernel (24 000 activations,
    /// 48 000 awaits): `is_registered_{,i32_,bool_}box_ptr` were 8.2 % + 5.9 %
    /// + 5.5 % of leaf samples.
    ///
    /// ## Why caching only positives is sound
    ///
    /// Box-cell memory is **never returned to the allocator**: an address
    /// minted by `js_*box_alloc*` is a box cell for the life of the thread —
    /// live in the registry, or (since the #7933 follow-up) parked in the
    /// release quarantine/free pool, but never recycled into a non-box
    /// allocation. `js_*box_release` removes a cell from the registry AND
    /// evicts it from this cache (`box_ptr_cache_evict`), so a cache hit
    /// still implies "currently registered": the only writer that removes a
    /// registry entry clears the matching cache slot in the same call, on
    /// the same thread. A hit is therefore exactly as authoritative as the
    /// probe it replaces.
    ///
    /// A **negative** cache would NOT be sound — an address that is not a box
    /// today can be minted as one tomorrow — so a miss always falls through to
    /// the hash set, and only a confirmed positive is recorded. That keeps the
    /// perry#4898 rejection (a read-only `__TEXT.__cstring` address that passes
    /// every structural check) exactly as strict as before.
    ///
    /// Thread-local like the registry it fronts: a box minted on another thread
    /// is not in this thread's registry, and never enters this thread's cache.
    static BOX_PTR_CACHE: [std::cell::Cell<usize>; BOX_PTR_CACHE_SLOTS] =
        const { [const { std::cell::Cell::new(0) }; BOX_PTR_CACHE_SLOTS] };
    static I32_BOX_PTR_CACHE: [std::cell::Cell<usize>; BOX_PTR_CACHE_SLOTS] =
        const { [const { std::cell::Cell::new(0) }; BOX_PTR_CACHE_SLOTS] };
    static BOOL_BOX_PTR_CACHE: [std::cell::Cell<usize>; BOX_PTR_CACHE_SLOTS] =
        const { [const { std::cell::Cell::new(0) }; BOX_PTR_CACHE_SLOTS] };
}

crate::perry_thread_local! {
    /// #7933 follow-up: reusable cells for each box kind, plus the fallback
    /// quarantine used by release calls made outside a tracked activation.
    ///
    /// `js_*box_release` names every cell in a completed plain-async frame.
    /// The async pump retains the activation token for queued/running steps.
    /// When those drain, each uncaptured cell clears and publishes while a
    /// closure-captured cell remains pending until its own capture count is
    /// zero. `js_*box_alloc*` pops the matching free list before touching
    /// `std::alloc`.
    ///
    /// ## Why the per-activation boundary is sound
    ///
    /// Before the activation reference count reaches zero, every terminal cell
    /// remains registered and unchanged because a queued/running resume can
    /// still observe the frame. At zero, the frame splits into independent
    /// cells: GC closure capture indexes follow moves and keep only the exact
    /// captured cells live until authoritative death pruning drops their final
    /// counts. Clearing and reuse therefore happen at each exact reachability
    /// boundary instead of one captured cell retaining the whole frame.
    ///
    /// Memory safety is unconditional either way: cells only ever move
    /// between the registry, the quarantine and the pool — they are never
    /// handed back to the allocator — so every address ever minted by
    /// `js_*box_alloc*` stays a valid box cell for the life of the thread.
    /// That preserves the two properties the never-free design bought:
    /// perry#4898's rejection of foreign pointers (an address is either a
    /// live registered cell or an inert parked one) and #7906's positive
    /// pointer cache ("was a box" can never become "is another object").
    ///
    /// NOT a GC root: published cells are cleared before parking, and the
    /// addresses themselves are `std::alloc` memory, not GC-heap pointers.
    /// The root-holder census intentionally does not classify bare core-crate
    /// integer tables of this shape; its documented rule-B limit applies.
    /// Head of the per-kind INTRUSIVE free list; 0 is the empty list.
    ///
    /// A free cell's own 8 bytes hold the address of the next free cell, so
    /// the reuse pool costs **zero** bytes of side table. That is not a
    /// micro-optimisation: a `Vec<usize>` pool is one 8-byte slot per cell on
    /// top of the cell, and the pool's high-water mark is ~330 cells per unit
    /// of PEAK CONCURRENCY (measured: `resident_cells / SIZE` is 329-334
    /// across a 16x sweep), held for the life of the thread. At SIZE=200 that
    /// side table was ~1 MB and made small async workloads a net RSS
    /// REGRESSION; threading the list through the cells removes it entirely.
    ///
    /// Overwriting the cell is why this list holds only cells that are past
    /// their activation's reachability boundary. Pending cells retain their
    /// real value while a closure can observe it; both the activation step
    /// boundary and that cell's capture count must be clear before its bytes
    /// become an intrusive link.
    static BOX_FREE_HEAD: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static I32_BOX_FREE_HEAD: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static BOOL_BOX_FREE_HEAD: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static BOX_RELEASE_QUARANTINE: std::cell::RefCell<Vec<usize>> =
        std::cell::RefCell::new(Vec::new());
    static I32_BOX_RELEASE_QUARANTINE: std::cell::RefCell<Vec<usize>> =
        std::cell::RefCell::new(Vec::new());
    static BOOL_BOX_RELEASE_QUARANTINE: std::cell::RefCell<Vec<usize>> =
        std::cell::RefCell::new(Vec::new());

    /// Stable non-GC activation tokens. Tokens are recycled rather than freed;
    /// a pending-await thunk captures the token pointer plus its generation and
    /// can therefore reject a stale capture without a per-activation HashMap.
    /// The control block contains no GC pointers and does not move when the
    /// collector relocates the activation's step closure or result Promise.
    static NEXT_ASYNC_BOX_ACTIVATION_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
    static ASYNC_BOX_ACTIVATION_FREE_HEAD: std::cell::Cell<*mut AsyncBoxActivation> =
        const { std::cell::Cell::new(std::ptr::null_mut()) };
    /// Tagged cell addresses released by tracked activations. An activation's
    /// entries are one contiguous range because `ReleaseBoxes` contains only
    /// adjacent, non-reentrant runtime calls. Published ranges become zeroed
    /// holes and trailing holes are popped, so capacity is bounded by delayed
    /// activations plus one releasing frame rather than process history.
    static ASYNC_RELEASED_CELLS: std::cell::RefCell<Vec<usize>> =
        const { std::cell::RefCell::new(Vec::new()) };
    /// Cells already named by a terminal `ReleaseBoxes`. They remain live and
    /// registered while an escaped closure can still read them. The value is
    /// the cell-kind tag plus `ASYNC_RELEASE_DRAINED` once queued/running step
    /// owners are gone; at that point a zero capture count publishes the cell.
    static ASYNC_PENDING_RELEASES: std::cell::RefCell<crate::fast_hash::PtrHashMap<usize, usize>> =
        std::cell::RefCell::new(crate::fast_hash::new_ptr_hash_map());
}

/// Malloc-side reachability token for one lowered plain-async activation.
///
/// `refs` counts the lifecycle owner plus queued/running async-step owners.
/// At zero, unobserved terminal cells publish immediately; a closure-captured
/// cell detaches from the activation and publishes independently when its own
/// capture count reaches zero.
pub(crate) struct AsyncBoxActivation {
    id: u64,
    refs: std::cell::Cell<usize>,
    lifecycle_owned: std::cell::Cell<bool>,
    release_start: std::cell::Cell<usize>,
    release_end: std::cell::Cell<usize>,
    next_free: std::cell::Cell<*mut AsyncBoxActivation>,
}

const NO_RELEASE_RANGE: usize = usize::MAX;
const ASYNC_RELEASE_JS: usize = 1;
const ASYNC_RELEASE_I32: usize = 2;
const ASYNC_RELEASE_BOOL: usize = 3;
const ASYNC_RELEASE_TAG_MASK: usize = 0b11;
const ASYNC_RELEASE_DRAINED: usize = 0b100;

/// Create the stable token for a plain-async activation. The activation
/// lifecycle owns the initial reference until a terminal release (or
/// `js_async_step_done`) marks the activation complete.
pub(crate) fn new_async_box_activation() -> *mut AsyncBoxActivation {
    let id = NEXT_ASYNC_BOX_ACTIVATION_ID.with(|next| {
        let id = next.get();
        // IDs are stored losslessly in a closure's f64 capture. Reaching 2^53
        // activations in one thread is not realistic; wrapping to 1 keeps 0 as
        // the permanent "not a tracked plain-async activation" sentinel.
        let following = if id >= (1u64 << 53) - 1 { 1 } else { id + 1 };
        next.set(following);
        id
    });
    let ptr = ASYNC_BOX_ACTIVATION_FREE_HEAD.with(|head| {
        let ptr = head.get();
        if ptr.is_null() {
            std::boxed::Box::into_raw(std::boxed::Box::new(AsyncBoxActivation {
                id,
                refs: std::cell::Cell::new(1),
                lifecycle_owned: std::cell::Cell::new(true),
                release_start: std::cell::Cell::new(NO_RELEASE_RANGE),
                release_end: std::cell::Cell::new(NO_RELEASE_RANGE),
                next_free: std::cell::Cell::new(std::ptr::null_mut()),
            }))
        } else {
            unsafe {
                head.set((*ptr).next_free.get());
                (*ptr).id = id;
                (*ptr).refs.set(1);
                (*ptr).lifecycle_owned.set(true);
                (*ptr).release_start.set(NO_RELEASE_RANGE);
                (*ptr).release_end.set(NO_RELEASE_RANGE);
                (*ptr).next_free.set(std::ptr::null_mut());
            }
            ptr
        }
    });
    ptr
}

#[inline]
pub(crate) fn async_box_activation_id(ptr: *mut AsyncBoxActivation) -> u64 {
    if ptr.is_null() {
        0
    } else {
        unsafe { (*ptr).id }
    }
}

/// Validate the stable token pointer + generation captured by a pending-await
/// thunk. Token storage is never freed, so reading it is safe even after the
/// token was recycled; the generation and lifecycle bit reject that stale
/// capture without a HashMap lookup on every async activation.
pub(crate) fn find_async_box_activation(
    ptr: *mut AsyncBoxActivation,
    id: u64,
) -> *mut AsyncBoxActivation {
    if ptr.is_null() || id == 0 {
        return std::ptr::null_mut();
    }
    unsafe {
        if (*ptr).id == id && (*ptr).lifecycle_owned.get() && (*ptr).refs.get() > 0 {
            ptr
        } else {
            std::ptr::null_mut()
        }
    }
}

#[inline]
pub(crate) fn retain_async_box_activation(ptr: *mut AsyncBoxActivation) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        let old = (*ptr).refs.get();
        debug_assert!(old > 0);
        (*ptr).refs.set(
            old.checked_add(1)
                .expect("async activation refcount overflow"),
        );
    }
}

/// Resolve a raw closure-capture word to a currently registered box address.
pub(crate) fn registered_box_capture_addr(addr: usize) -> Option<usize> {
    if !is_plausible_box_ptr(addr as *mut Box) {
        return None;
    }
    let ptr = addr as *mut Box;
    let is_live_box = is_registered_box_ptr(ptr)
        || is_registered_i32_box_ptr(ptr.cast::<I32Box>())
        || is_registered_bool_box_ptr(ptr.cast::<BoolBox>());
    is_live_box.then_some(addr)
}

#[inline]
pub(crate) fn release_async_box_activation(ptr: *mut AsyncBoxActivation) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        let old = (*ptr).refs.get();
        debug_assert!(old > 0);
        let new = old - 1;
        (*ptr).refs.set(new);
        if new == 0 {
            debug_assert!(!(*ptr).lifecycle_owned.get());
            publish_async_activation_cells(ptr);
            ASYNC_BOX_ACTIVATION_FREE_HEAD.with(|head| {
                (*ptr).next_free.set(head.get());
                head.set(ptr);
            });
        }
    }
}

fn park_async_activation_cell(activation: *mut AsyncBoxActivation, addr: usize, tag: usize) {
    debug_assert!(!activation.is_null());
    debug_assert_eq!(addr & ASYNC_RELEASE_TAG_MASK, 0);
    debug_assert!((ASYNC_RELEASE_JS..=ASYNC_RELEASE_BOOL).contains(&tag));
    ASYNC_RELEASED_CELLS.with(|cells| {
        let mut cells = cells.borrow_mut();
        unsafe {
            let start = (*activation).release_start.get();
            if start == NO_RELEASE_RANGE {
                (*activation).release_start.set(cells.len());
            } else {
                debug_assert_eq!(
                    (*activation).release_end.get(),
                    cells.len(),
                    "ReleaseBoxes calls for one activation must be contiguous"
                );
            }
            cells.push(addr | tag);
            (*activation).release_end.set(cells.len());
        }
    });
}

fn push_free_cell(addr: usize, head: &'static crate::tls_hot::HotKey<std::cell::Cell<usize>>) {
    head.with(|head| {
        let next = head.get();
        debug_assert_eq!(addr % ALIGN_OF_BOX_CELL, 0);
        unsafe { (addr as *mut usize).write(next) };
        head.set(addr);
    });
}

fn publish_box_cell(addr: usize, tag: usize) {
    match tag {
        ASYNC_RELEASE_JS => {
            BOX_REGISTRY.with(|r| {
                r.borrow_mut().remove(&addr);
            });
            box_ptr_cache_evict(&BOX_PTR_CACHE, addr);
            unsafe { (*(addr as *mut Box)).value = crate::value::TAG_UNDEFINED };
            push_free_cell(addr, &BOX_FREE_HEAD);
        }
        ASYNC_RELEASE_I32 => {
            I32_BOX_REGISTRY.with(|r| {
                r.borrow_mut().remove(&addr);
            });
            box_ptr_cache_evict(&I32_BOX_PTR_CACHE, addr);
            unsafe { (*(addr as *mut I32Box)).value = -1 };
            push_free_cell(addr, &I32_BOX_FREE_HEAD);
        }
        ASYNC_RELEASE_BOOL => {
            BOOL_BOX_REGISTRY.with(|r| {
                r.borrow_mut().remove(&addr);
            });
            box_ptr_cache_evict(&BOOL_BOX_PTR_CACHE, addr);
            unsafe { (*(addr as *mut BoolBox)).value = true };
            push_free_cell(addr, &BOOL_BOX_FREE_HEAD);
        }
        _ => unreachable!("invalid async released-cell tag"),
    }
    ASYNC_PENDING_RELEASES.with(|pending| {
        pending.borrow_mut().remove(&addr);
    });
    BOX_FLUSH_PUBLISHED.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn box_capture_count_reached_zero(addr: usize) {
    let pending = ASYNC_PENDING_RELEASES
        .with(|releases| releases.borrow().get(&addr).copied())
        .unwrap_or(0);
    if pending & ASYNC_RELEASE_DRAINED != 0 {
        publish_box_cell(addr, pending & ASYNC_RELEASE_TAG_MASK);
    }
}

/// Expose a drained, closure-owned JS box's payload to the closure tracer.
/// The exact-capture table may also contain i32/bool box addresses; requiring
/// the pending JS tag is the authoritative type discriminator before the
/// pointer is dereferenced as [`Box`].
pub(crate) fn visit_pending_captured_js_box_payload_slot(
    addr: usize,
    visit: &mut dyn FnMut(*mut u64),
) {
    let is_pending_js = ASYNC_PENDING_RELEASES.with(|pending| {
        pending
            .borrow()
            .get(&addr)
            .is_some_and(|tag| *tag == (ASYNC_RELEASE_JS | ASYNC_RELEASE_DRAINED))
    });
    if is_pending_js && BOX_REGISTRY.with(|registry| registry.borrow().contains(&addr)) {
        let ptr = addr as *mut Box;
        unsafe { visit(&raw mut (*ptr).value) };
    }
}

fn begin_pending_release(addr: usize, tag: usize) -> bool {
    ASYNC_PENDING_RELEASES.with(|pending| {
        let mut pending = pending.borrow_mut();
        if pending.contains_key(&addr) {
            false
        } else {
            pending.insert(addr, tag);
            true
        }
    })
}

fn publish_async_activation_cells(activation: *mut AsyncBoxActivation) {
    let (start, end) = unsafe {
        (
            (*activation).release_start.get(),
            (*activation).release_end.get(),
        )
    };
    if start == NO_RELEASE_RANGE {
        return;
    }
    ASYNC_RELEASED_CELLS.with(|cells| {
        let mut cells = cells.borrow_mut();
        debug_assert!(start <= end && end <= cells.len());
        for tagged in &mut cells[start..end] {
            let value = *tagged;
            if value == 0 {
                continue;
            }
            let addr = value & !ASYNC_RELEASE_TAG_MASK;
            let tag = value & ASYNC_RELEASE_TAG_MASK;
            if crate::closure::box_capture_count(addr) == 0 {
                publish_box_cell(addr, tag);
            } else {
                ASYNC_PENDING_RELEASES.with(|pending| {
                    let previous = pending
                        .borrow_mut()
                        .insert(addr, tag | ASYNC_RELEASE_DRAINED);
                    debug_assert_eq!(previous, Some(tag));
                });
            }
            *tagged = 0;
        }
        while cells.last() == Some(&0) {
            cells.pop();
        }
    });
}

/// Drop the activation lifecycle's owner at terminal state. Queued or running
/// steps keep their own references; the cells publish only when the last of
/// those references is released. `replace(false)` makes duplicate terminal
/// releases idempotent without a per-activation registry lookup.
pub(crate) fn finish_async_box_activation(ptr: *mut AsyncBoxActivation) {
    if ptr.is_null() {
        return;
    }
    if unsafe { (*ptr).lifecycle_owned.replace(false) } {
        release_async_box_activation(ptr);
    }
}

fn publish_released_cells(
    cells: &mut Vec<usize>,
    head: &'static crate::tls_hot::HotKey<std::cell::Cell<usize>>,
) {
    if cells.is_empty() {
        return;
    }
    BOX_FLUSH_PUBLISHED.fetch_add(cells.len() as u64, Ordering::Relaxed);
    head.with(|h| {
        let mut next = h.get();
        for addr in cells.drain(..) {
            debug_assert_eq!(addr % ALIGN_OF_BOX_CELL, 0);
            unsafe { (addr as *mut usize).write(next) };
            next = addr;
        }
        h.set(next);
    });
}

/// Drain the release quarantines into the free pools. Called at the
/// outermost microtask-pump exit once TASK_QUEUE is empty (see the
/// QUARANTINE doc above for why that boundary), and by tests.
pub fn flush_released_boxes() {
    BOX_FLUSH_COUNT.fetch_add(1, Ordering::Relaxed);
    for (q, head) in [
        (&BOX_RELEASE_QUARANTINE, &BOX_FREE_HEAD),
        (&I32_BOX_RELEASE_QUARANTINE, &I32_BOX_FREE_HEAD),
        (&BOOL_BOX_RELEASE_QUARANTINE, &BOOL_BOX_FREE_HEAD),
    ] {
        q.with(|q| {
            let mut q = q.borrow_mut();
            if q.is_empty() {
                return;
            }
            publish_released_cells(&mut q, head);
            // Deliberately NOT shrunk. The quarantine is refilled to roughly
            // the same size every interval, so handing the buffer back here
            // just makes the next interval re-grow it: measured, shrinking to
            // 1 KiB each flush cost +5.3 MB peak RSS at BATCHES=1200 in
            // allocator churn, which is the opposite of the point.
        });
    }
}

/// Every box cell is exactly one pointer wide, which is what lets the free
/// list live inside the cells. Asserted rather than assumed: a field added to
/// any box struct would silently make the link write out of bounds.
const ALIGN_OF_BOX_CELL: usize = std::mem::align_of::<Box>();
const _: () = {
    assert!(std::mem::size_of::<Box>() == std::mem::size_of::<usize>());
    assert!(std::mem::size_of::<I32Box>() == std::mem::size_of::<usize>());
    assert!(std::mem::size_of::<BoolBox>() == std::mem::size_of::<usize>());
    assert!(std::mem::align_of::<Box>() >= std::mem::align_of::<usize>());
    assert!(std::mem::align_of::<I32Box>() >= std::mem::align_of::<usize>());
    assert!(std::mem::align_of::<BoolBox>() >= std::mem::align_of::<usize>());
};

/// Pop a cell from an intrusive free list, or 0 when it is empty.
#[inline(always)]
fn pop_free_cell(head: &'static crate::tls_hot::HotKey<std::cell::Cell<usize>>) -> usize {
    head.with(|h| {
        let addr = h.get();
        if addr != 0 {
            // SAFETY: `addr` was minted by `js_*box_alloc*`, is cell-sized and
            // cell-aligned, and its memory is never returned to the allocator,
            // so the link written at publish time is still there.
            h.set(unsafe { (addr as *const usize).read() });
        }
        addr
    })
}

/// Boxes are 8-byte allocations, so bits 0..3 of an address carry no
/// information; index on the bits above them.
#[inline(always)]
fn box_ptr_cache_index(addr: usize) -> usize {
    (addr >> 3) & (BOX_PTR_CACHE_SLOTS - 1)
}

#[inline(always)]
fn box_ptr_cache_hit(cache: &'static BoxPtrCache, addr: usize) -> bool {
    cache.with(|slots| slots[box_ptr_cache_index(addr)].get() == addr)
}

#[inline(always)]
fn box_ptr_cache_record(cache: &'static BoxPtrCache, addr: usize) {
    cache.with(|slots| slots[box_ptr_cache_index(addr)].set(addr));
}

/// Evict `addr` from its direct-mapped cache slot if it currently occupies
/// it. Called on publication so a parked cell is invisible to the positive cache
/// too — the parked-cell inertness argument in the QUARANTINE doc relies on
/// every `js_box_get`/`js_box_set` on a parked address falling through to
/// the registry probe and missing.
#[inline(always)]
fn box_ptr_cache_evict(cache: &'static BoxPtrCache, addr: usize) {
    cache.with(|slots| {
        let slot = &slots[box_ptr_cache_index(addr)];
        if slot.get() == addr {
            slot.set(0);
        }
    });
}

/// Allocate a new box with an initial JSValue bit pattern.
#[no_mangle]
pub extern "C" fn js_box_alloc_bits(initial_bits: i64) -> *mut Box {
    BOX_ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
    // #7933 follow-up: serve from the free pool first. A pooled address was
    // minted by this function, cleared and de-registered at release, and is
    // provably unreferenced (see the QUARANTINE doc) — re-registering it
    // with a fresh value is indistinguishable from a fresh allocation.
    let pooled = pop_free_cell(&BOX_FREE_HEAD);
    if pooled != 0 {
        let addr = pooled;
        BOX_POOL_REUSE_COUNT.fetch_add(1, Ordering::Relaxed);
        let ptr = addr as *mut Box;
        unsafe {
            (*ptr).value = initial_bits as u64;
        }
        BOX_REGISTRY.with(|r| {
            r.borrow_mut().insert(addr);
        });
        box_ptr_cache_record(&BOX_PTR_CACHE, addr);
        return ptr;
    }
    unsafe {
        let layout = Layout::new::<Box>();
        let ptr = alloc(layout) as *mut Box;
        if ptr.is_null() {
            // perry#924: oom is rare enough that operators see the
            // downstream crash and react to that; keep the diagnostic
            // available under `PERRY_DEBUG=1` for bisection.
            if std::env::var_os("PERRY_DEBUG").is_some() {
                eprintln!("[PERRY WARN] js_box_alloc: allocation failed — returning null");
            }
            return std::ptr::null_mut();
        }
        (*ptr).value = initial_bits as u64;
        BOX_REGISTRY.with(|r| {
            r.borrow_mut().insert(ptr as usize);
        });
        box_ptr_cache_record(&BOX_PTR_CACHE, ptr as usize);
        ptr
    }
}

/// Compatibility wrapper for legacy f64-lowered boxed locals.
#[no_mangle]
pub extern "C" fn js_box_alloc(initial_value: f64) -> *mut Box {
    js_box_alloc_bits(initial_value.to_bits() as i64)
}

#[no_mangle]
pub extern "C" fn js_i32_box_alloc(initial_value: i32) -> *mut I32Box {
    BOX_ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
    let pooled = pop_free_cell(&I32_BOX_FREE_HEAD);
    if pooled != 0 {
        let addr = pooled;
        BOX_POOL_REUSE_COUNT.fetch_add(1, Ordering::Relaxed);
        let ptr = addr as *mut I32Box;
        unsafe {
            (*ptr).value = initial_value;
        }
        I32_BOX_REGISTRY.with(|r| {
            r.borrow_mut().insert(addr);
        });
        box_ptr_cache_record(&I32_BOX_PTR_CACHE, addr);
        return ptr;
    }
    unsafe {
        let layout = Layout::new::<I32Box>();
        let ptr = alloc(layout) as *mut I32Box;
        if ptr.is_null() {
            if std::env::var_os("PERRY_DEBUG").is_some() {
                eprintln!("[PERRY WARN] js_i32_box_alloc: allocation failed — returning null");
            }
            return std::ptr::null_mut();
        }
        (*ptr).value = initial_value;
        I32_BOX_REGISTRY.with(|r| {
            r.borrow_mut().insert(ptr as usize);
        });
        box_ptr_cache_record(&I32_BOX_PTR_CACHE, ptr as usize);
        ptr
    }
}

#[no_mangle]
pub extern "C" fn js_bool_box_alloc(initial_value: i32) -> *mut BoolBox {
    BOX_ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
    let pooled = pop_free_cell(&BOOL_BOX_FREE_HEAD);
    if pooled != 0 {
        let addr = pooled;
        BOX_POOL_REUSE_COUNT.fetch_add(1, Ordering::Relaxed);
        let ptr = addr as *mut BoolBox;
        unsafe {
            (*ptr).value = initial_value != 0;
        }
        BOOL_BOX_REGISTRY.with(|r| {
            r.borrow_mut().insert(addr);
        });
        box_ptr_cache_record(&BOOL_BOX_PTR_CACHE, addr);
        return ptr;
    }
    unsafe {
        let layout = Layout::new::<BoolBox>();
        let ptr = alloc(layout) as *mut BoolBox;
        if ptr.is_null() {
            if std::env::var_os("PERRY_DEBUG").is_some() {
                eprintln!("[PERRY WARN] js_bool_box_alloc: allocation failed — returning null");
            }
            return std::ptr::null_mut();
        }
        (*ptr).value = initial_value != 0;
        BOOL_BOX_REGISTRY.with(|r| {
            r.borrow_mut().insert(ptr as usize);
        });
        box_ptr_cache_record(&BOOL_BOX_PTR_CACHE, ptr as usize);
        ptr
    }
}

/// #7933 follow-up: release one JSValue box cell at a plain-async
/// activation's terminal state.
///
/// Emitted by codegen for every cell in a completed plain-async frame. It
/// records the cell in the activation's pending terminal release range.
/// Clearing, de-registration and reuse wait for both async-step references and
/// GC closures capturing the cell to disappear (see the pool doc above).
///
/// Idempotent and foreign-pointer-safe by the same gate: a pointer that is
/// not currently registered — already released, never a box, or a
/// perry#4898-style bogus address — is left untouched. The terminal arms of
/// the step machine can re-run on a stray duplicate resume, so double
/// release MUST be a total no-op (a second park of the same address would
/// alias two future activations onto one cell).
#[no_mangle]
pub extern "C" fn js_box_release(ptr: *mut Box) {
    let addr = ptr as usize;
    if !is_plausible_box_ptr(ptr) {
        return;
    }
    let activation = crate::promise::current_async_box_activation();
    if !activation.is_null() {
        if !BOX_REGISTRY.with(|r| r.borrow().contains(&addr)) {
            return;
        }
        if !begin_pending_release(addr, ASYNC_RELEASE_JS) {
            return;
        }
        park_async_activation_cell(activation, addr, ASYNC_RELEASE_JS);
        finish_async_box_activation(activation);
        BOX_RELEASE_COUNT.fetch_add(1, Ordering::Relaxed);
        return;
    }
    let was_registered = BOX_REGISTRY.with(|r| r.borrow_mut().remove(&addr));
    if !was_registered {
        return;
    }
    box_ptr_cache_evict(&BOX_PTR_CACHE, addr);
    unsafe {
        // Cleared BEFORE parking: a parked cell must read as `undefined`
        // through any stale path, and must retain nothing for the GC (the
        // root scanner only walks the registry, which no longer has it).
        (*ptr).value = crate::value::TAG_UNDEFINED;
    }
    BOX_RELEASE_QUARANTINE.with(|q| q.borrow_mut().push(addr));
    BOX_RELEASE_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// `js_box_release` for the compiler-private i32 control cells
/// (`__gen_state` / `__gen_pending_type`). Same contract as
/// `js_box_release`, with one twist: generated async-step code reads these
/// cells with RAW inline loads (`load_async_i32_control_cell`), never
/// through the registry-checked getter — so the PARKED VALUE is what a stray
/// duplicate resume would observe. Park `-1`: the linearizer numbers states
/// from 0, so `-1` matches no dispatch case and no catch-route condition,
/// and the dispatch loop's default arm returns the done iter-result.
/// (Unreachable in practice anyway: the parked `__gen_done = true` below
/// short-circuits a stray resume before any state read.)
#[no_mangle]
pub extern "C" fn js_i32_box_release(ptr: *mut I32Box) {
    let addr = ptr as usize;
    if !is_plausible_box_ptr(ptr.cast::<Box>()) {
        return;
    }
    let activation = crate::promise::current_async_box_activation();
    if !activation.is_null() {
        if !I32_BOX_REGISTRY.with(|r| r.borrow().contains(&addr)) {
            return;
        }
        if !begin_pending_release(addr, ASYNC_RELEASE_I32) {
            return;
        }
        park_async_activation_cell(activation, addr, ASYNC_RELEASE_I32);
        finish_async_box_activation(activation);
        BOX_RELEASE_COUNT.fetch_add(1, Ordering::Relaxed);
        return;
    }
    let was_registered = I32_BOX_REGISTRY.with(|r| r.borrow_mut().remove(&addr));
    if !was_registered {
        return;
    }
    box_ptr_cache_evict(&I32_BOX_PTR_CACHE, addr);
    unsafe {
        (*ptr).value = -1;
    }
    I32_BOX_RELEASE_QUARANTINE.with(|q| q.borrow_mut().push(addr));
    BOX_RELEASE_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// `js_box_release` for the compiler-private i1 control cells
/// (`__gen_done` / `__gen_executing`). Same contract. Like the i32 cells,
/// generated code reads these with RAW inline loads, so the parked value is
/// what a stray duplicate resume observes. Park `true`: a stray resume's
/// first control read is `if (__gen_done) return {done: true}` — parked
/// `true` takes byte-for-byte the pre-release terminal short-circuit
/// (`__gen_done` really was `true` when the activation completed).
/// `__gen_executing` also parks `true`, which is fine: the executing guard
/// belongs to the user-callable generator `.next()` wrappers, and
/// generators never release (only `was_plain_async` activations do, and
/// their fused step body never reads `__gen_executing`).
#[no_mangle]
pub extern "C" fn js_bool_box_release(ptr: *mut BoolBox) {
    let addr = ptr as usize;
    if !is_plausible_box_ptr(ptr.cast::<Box>()) {
        return;
    }
    let activation = crate::promise::current_async_box_activation();
    if !activation.is_null() {
        if !BOOL_BOX_REGISTRY.with(|r| r.borrow().contains(&addr)) {
            return;
        }
        if !begin_pending_release(addr, ASYNC_RELEASE_BOOL) {
            return;
        }
        park_async_activation_cell(activation, addr, ASYNC_RELEASE_BOOL);
        finish_async_box_activation(activation);
        BOX_RELEASE_COUNT.fetch_add(1, Ordering::Relaxed);
        return;
    }
    let was_registered = BOOL_BOX_REGISTRY.with(|r| r.borrow_mut().remove(&addr));
    if !was_registered {
        return;
    }
    box_ptr_cache_evict(&BOOL_BOX_PTR_CACHE, addr);
    unsafe {
        (*ptr).value = true;
    }
    BOOL_BOX_RELEASE_QUARANTINE.with(|q| q.borrow_mut().push(addr));
    BOX_RELEASE_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// GC root scanner: walk every registered box and `mark` the JSValue bit
/// value inside. Heap pointers stored inside boxes (e.g. the generator
/// state machine's iter object held in a mutable-capture box) must be
/// kept alive across collections. The box pointer itself is _not_ a
/// heap value the runtime tracks — `BOX_REGISTRY` is the source of
/// truth for "every live box right now" — so we use the standard root
/// scanner protocol: dispatch every stored JSValue bit pattern to `mark`
/// and let the GC trace into it.
pub fn scan_box_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_box_roots_mut(&mut visitor);
}

pub fn scan_box_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let full_trace = crate::gc::full_trace_active();
    ASYNC_PENDING_RELEASES.with(|pending| {
        let pending = pending.borrow();
        BOX_REGISTRY.with(|r| {
            let r = r.borrow();
            for &addr in r.iter() {
                // A drained box is retained only by exact closure-capture
                // metadata. During a full trace its payload is reached from
                // each live closure instead. Rooting it here as well would
                // make `box -> closure -> same box` an uncollectable native
                // cycle. Minors retain the old strong-root rule because they
                // cannot adjudicate old-closure liveness.
                if full_trace
                    && pending
                        .get(&addr)
                        .is_some_and(|tag| *tag == (ASYNC_RELEASE_JS | ASYNC_RELEASE_DRAINED))
                {
                    continue;
                }
                let ptr = addr as *mut Box;
                // Defensive: the registry should only contain valid live
                // pointers, but if a stale entry slipped through we'd
                // segfault on the deref. The tight bounds check on the
                // address (alloc gives 8-aligned pointers in user space)
                // matches `is_plausible_box_ptr` to keep this a no-op for
                // any pathological entry.
                if addr >= 0x1000 && (addr as u64) < 0x0001_0000_0000_0000 && addr % 8 == 0 {
                    unsafe {
                        visitor.visit_nanbox_u64_raw_slot(&raw mut (*ptr).value);
                    }
                }
            }
        });
    });
}

/// Get the raw JSValue bit pattern from a box.
///
/// Same robustness as `js_box_set`: invalid pointers return `undefined`
/// rather than dereferencing. See perry#393 for the failure mode.
#[no_mangle]
pub extern "C" fn js_box_get_bits(ptr: *mut Box) -> i64 {
    unsafe {
        if !is_registered_box_ptr(ptr) {
            // perry#924: production services see these in tight bursts of
            // 3 synced with normal request handling and the operator can't
            // tell whether anything is wrong. The path is correctness-safe
            // (we already return a defined value to the caller); gate the
            // diagnostic behind `PERRY_DEBUG=1` so it only surfaces during
            // bisection.
            if std::env::var_os("PERRY_DEBUG").is_some() {
                let count = BOX_GET_NULL_COUNT.fetch_add(1, Ordering::Relaxed);
                if count < 3 {
                    eprintln!(
                        "[PERRY WARN] js_box_get: invalid box pointer {:p} #{}",
                        ptr, count
                    );
                }
            }
            // perry#4926: with codegen entry-initializing boxed slots to
            // TAG_UNDEFINED, this arm is the read-before-initialization
            // path for a boxed variable — in JS that reads as `undefined`
            // (Perry has no TDZ), not as the number NaN. TAG_UNDEFINED is
            // itself a quiet-NaN bit pattern, so numeric consumers behave
            // exactly as before; JS-level checks (`typeof`, `== null`)
            // now see `undefined`.
            return crate::value::TAG_UNDEFINED as i64;
        }
        let bits = (*ptr).value;
        // Temporal Dead Zone: a lexical `let`/`const`/`class` box seeded with
        // the TAG_TDZ sentinel at scope entry throws a spec ReferenceError when
        // read before its declaration runs (which overwrites the sentinel with
        // a real value). TAG_TDZ is a reserved bit pattern no legitimate value
        // ever holds, so this branch is only ever taken on a genuine
        // read-before-initialization — making the check zero-regression for
        // every already-initialized box. The name is passed as `undefined`
        // because this choke point is name-agnostic (it serves direct,
        // closure-captured, and compound reads alike); the resulting message is
        // the spec-generic form.
        if bits == crate::value::TAG_TDZ {
            // #6044 regression (#6052): Perry-internal materialization reads —
            // the class-capture decl-site snapshot refreshes emitted after EACH
            // captured var's assignment (`RegisterClassCaptures`, the #6037
            // refresh strategy) — legally observe sibling captures that are
            // still in their dead zone (`const _fs = ..; <refresh reads _path>;
            // const _path = ..`, the SWC CJS interop shape). Those are not user
            // reads: pre-TDZ they snapshotted `undefined` and the next refresh
            // fixed the value up. Inside the codegen-bracketed suppression
            // window, keep exactly that behavior instead of throwing.
            if TDZ_SUPPRESS_DEPTH.with(|d| d.get()) > 0 {
                return crate::value::TAG_UNDEFINED as i64;
            }
            crate::error::js_throw_reference_error_tdz(f64::from_bits(crate::value::TAG_UNDEFINED));
        }
        bits as i64
    }
}

crate::perry_thread_local! {
    /// #6052: >0 while codegen-emitted Perry-internal materialization reads
    /// (the `RegisterClassCaptures` decl-site snapshot refresh) are running —
    /// a dead-zone box then reads as `undefined` (pre-#6044 behavior) instead
    /// of throwing. Never spans user code: the bracketed window contains only
    /// side-effect-free capture loads.
    static TDZ_SUPPRESS_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Enter a TDZ-suppression window (see `TDZ_SUPPRESS_DEPTH`). Emitted by
/// codegen immediately before a `RegisterClassCaptures` snapshot's capture
/// loads; paired with `js_tdz_suppress_end`.
#[no_mangle]
pub extern "C" fn js_tdz_suppress_begin() {
    TDZ_SUPPRESS_DEPTH.with(|d| d.set(d.get().saturating_add(1)));
}

/// Leave the TDZ-suppression window opened by `js_tdz_suppress_begin`.
#[no_mangle]
pub extern "C" fn js_tdz_suppress_end() {
    TDZ_SUPPRESS_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
}

/// Keepalive anchors for the auto-optimize whole-program build (generated-code-
/// only callees — without these the symbols dead-strip and the app link fails).
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_TDZ_SUPPRESS_BEGIN: extern "C" fn() = js_tdz_suppress_begin;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_TDZ_SUPPRESS_END: extern "C" fn() = js_tdz_suppress_end;

/// Compatibility wrapper for legacy f64-lowered boxed locals.
#[no_mangle]
pub extern "C" fn js_box_get(ptr: *mut Box) -> f64 {
    f64::from_bits(js_box_get_bits(ptr) as u64)
}

#[no_mangle]
pub extern "C" fn js_i32_box_get(ptr: *mut I32Box) -> i32 {
    unsafe {
        if !is_registered_i32_box_ptr(ptr) {
            if std::env::var_os("PERRY_DEBUG").is_some() {
                let count = I32_BOX_GET_NULL_COUNT.fetch_add(1, Ordering::Relaxed);
                if count < 3 {
                    eprintln!(
                        "[PERRY WARN] js_i32_box_get: invalid box pointer {:p} #{}",
                        ptr, count
                    );
                }
            }
            return 0;
        }
        (*ptr).value
    }
}

#[no_mangle]
pub extern "C" fn js_bool_box_get(ptr: *mut BoolBox) -> i32 {
    unsafe {
        if !is_registered_bool_box_ptr(ptr) {
            if std::env::var_os("PERRY_DEBUG").is_some() {
                let count = BOOL_BOX_GET_NULL_COUNT.fetch_add(1, Ordering::Relaxed);
                if count < 3 {
                    eprintln!(
                        "[PERRY WARN] js_bool_box_get: invalid box pointer {:p} #{}",
                        ptr, count
                    );
                }
            }
            return 0;
        }
        i32::from((*ptr).value)
    }
}

/// Set the raw JSValue bit pattern in a box.
///
/// Robust against bogus pointers: in addition to the null check, we
/// reject obviously-invalid pointers (below the first user page or
/// above the 48-bit user-address ceiling) and pointers that aren't
/// 8-byte aligned. This avoids SIGSEGV on `(*ptr).value = value` when
/// upstream codegen hands us a stale/uninitialized slot — a known
/// failure mode for closure prologues at hub-scale (perry#393).
/// Boxes are heap-allocated 8-byte JSValue bit slots; a non-aligned or low/high
/// pointer is definitely wrong, so a silent skip + telemetry warning
/// is strictly safer than dereferencing it.
#[no_mangle]
pub extern "C" fn js_box_set_bits(ptr: *mut Box, value_bits: i64) {
    unsafe {
        if !is_registered_box_ptr(ptr) {
            // perry#924: silent-skip is correctness-safe (caller's box
            // mutation is dropped, which is the same as no closure
            // capture having existed). Gate diagnostics behind
            // `PERRY_DEBUG=1` to keep production stderr clean.
            if std::env::var_os("PERRY_DEBUG").is_some() {
                let count = BOX_SET_NULL_COUNT.fetch_add(1, Ordering::Relaxed);
                if count < 3 {
                    eprintln!(
                        "[PERRY WARN] js_box_set: invalid box pointer {:p} #{} (value bits: 0x{:016x})",
                        ptr,
                        count,
                        value_bits as u64
                    );
                }
            }
            return;
        }
        let bits = value_bits as u64;
        (*ptr).value = bits;
        crate::gc::runtime_write_barrier_root_nanbox(bits);
    }
}

/// Compatibility wrapper for legacy f64-lowered boxed locals.
#[no_mangle]
pub extern "C" fn js_box_set(ptr: *mut Box, value: f64) {
    js_box_set_bits(ptr, value.to_bits() as i64);
}

#[no_mangle]
pub extern "C" fn js_i32_box_set(ptr: *mut I32Box, value: i32) {
    unsafe {
        if !is_registered_i32_box_ptr(ptr) {
            if std::env::var_os("PERRY_DEBUG").is_some() {
                let count = I32_BOX_SET_NULL_COUNT.fetch_add(1, Ordering::Relaxed);
                if count < 3 {
                    eprintln!(
                        "[PERRY WARN] js_i32_box_set: invalid box pointer {:p} #{} (value: {})",
                        ptr, count, value
                    );
                }
            }
            return;
        }
        (*ptr).value = value;
    }
}

#[no_mangle]
pub extern "C" fn js_bool_box_set(ptr: *mut BoolBox, value: i32) {
    unsafe {
        if !is_registered_bool_box_ptr(ptr) {
            if std::env::var_os("PERRY_DEBUG").is_some() {
                let count = BOOL_BOX_SET_NULL_COUNT.fetch_add(1, Ordering::Relaxed);
                if count < 3 {
                    eprintln!(
                        "[PERRY WARN] js_bool_box_set: invalid box pointer {:p} #{} (value: {})",
                        ptr, count, value
                    );
                }
            }
            return;
        }
        (*ptr).value = value != 0;
    }
}

/// Cheap pointer-sanity test — same threat model as `get_valid_func_ptr`
/// in closure.rs, adapted for box-shaped allocations.
///
/// A `*mut Box` from `js_box_alloc` is a Rust-`alloc()` heap pointer,
/// which on x86_64 Linux/macOS lives in the 47-bit user-address half
/// of the address space and (because `Layout::new::<Box>()` yields
/// `align = 8`) is 8-byte aligned. Pointers below the first user page
/// or above the user-address ceiling, or unaligned ones, can only come
/// from stale/uninitialized stack slots reinterpreted as box pointers.
///
/// perry#4898: the structural checks are necessary but **not sufficient**.
/// A miscompiled `js_box_set` can be handed a box-pointer operand that was
/// effectively `undef`/poison at the IR level (e.g. a mutable-capture box
/// whose allocation was elided on the taken path). LLVM then fills the
/// register with whatever was conveniently live — under typed-feedback
/// (#854) instrumentation that is the read-only `..._guard` string constant
/// passed to `js_typed_feedback_register_site`. That constant is ≥0x1000,
/// untagged (top-16 zero), and 8-byte aligned, so it sails through every
/// structural check — and `(*ptr).value = value` then writes into
/// `__TEXT.__cstring`, a SIGBUS. The address `read_static`-looks like a box
/// but isn't one. `is_registered_box_ptr` closes that gap: a pointer that
/// `js_box_alloc` never minted is rejected before the deref.
#[inline]
fn is_plausible_box_ptr(ptr: *mut Box) -> bool {
    let addr = ptr as usize;
    if addr == 0 {
        return false;
    }
    if addr < 0x1000 {
        return false;
    }
    if (addr as u64) >= 0x0001_0000_0000_0000 {
        return false;
    }
    if !addr.is_multiple_of(std::mem::align_of::<Box>()) {
        return false;
    }
    true
}

/// Authoritative box-pointer check: the address must have been minted by
/// `js_box_alloc` and be currently registered. Box-cell memory is never
/// returned to the allocator — a cell is live-registered or parked in the
/// release pool (#7933 follow-up), never recycled into a non-box
/// allocation — so membership has no false negatives for a live box and no
/// stale-reuse hazard: an address that isn't in the registry is either not
/// a box at all or a released (inert, `undefined`-reading) cell, and
/// treating both as "not a box" is exactly right. This is what stops a
/// stray read-only/garbage pointer (perry#4898) from being dereferenced as
/// a box, and what makes a parked cell's reads/writes inert.
#[inline]
fn is_registered_box_ptr(ptr: *mut Box) -> bool {
    if !is_plausible_box_ptr(ptr) {
        return false;
    }
    let addr = ptr as usize;
    if box_ptr_cache_hit(&BOX_PTR_CACHE, addr) {
        return true;
    }
    let present = BOX_REGISTRY.with(|r| r.borrow().contains(&addr));
    if present {
        box_ptr_cache_record(&BOX_PTR_CACHE, addr);
    }
    present
}

/// If `slot_bits` (the raw contents of a closure capture slot) is a registered
/// box pointer, return the JSValue bits stored *inside* that box; otherwise
/// return `None`.
///
/// A closure that captures a boxed local — every body local of an `async`
/// function (the async-to-generator transform boxes them all), plus any
/// mutable capture — stores the raw box pointer in its capture slot rather
/// than a NaN-boxed value (see the codegen closure lowering in
/// `perry-codegen/src/expr/closure.rs`). That pointer addresses a box in the
/// *current thread's* thread-local `BOX_REGISTRY`, so it is
/// meaningless on any other thread. The `perry/thread` serializer uses this to
/// unwrap such a slot to the value the box actually holds before deep-copying
/// it across the boundary (#6520 — without it the worker read the captured
/// value as `undefined`/empty).
///
/// Registry membership is authoritative: any NaN-boxed value or real double
/// has its high bits set and fails `is_plausible_box_ptr`, so this only ever
/// matches a genuine live box pointer, never a coincidental capture value.
#[inline]
pub fn box_slot_contents_bits(slot_bits: u64) -> Option<u64> {
    let ptr = slot_bits as usize as *mut Box;
    if is_registered_box_ptr(ptr) {
        // Safety: the address is in BOX_REGISTRY, so it was minted by
        // `js_box_alloc` and points at a live `Box` (cell memory is never
        // returned to the allocator; see the release-pool doc).
        Some(unsafe { (*ptr).value })
    } else {
        None
    }
}

#[inline]
fn is_registered_i32_box_ptr(ptr: *mut I32Box) -> bool {
    if !is_plausible_box_ptr(ptr.cast::<Box>()) {
        return false;
    }
    let addr = ptr as usize;
    if box_ptr_cache_hit(&I32_BOX_PTR_CACHE, addr) {
        return true;
    }
    let present = I32_BOX_REGISTRY.with(|r| r.borrow().contains(&addr));
    if present {
        box_ptr_cache_record(&I32_BOX_PTR_CACHE, addr);
    }
    present
}

#[inline]
fn is_registered_bool_box_ptr(ptr: *mut BoolBox) -> bool {
    if !is_plausible_box_ptr(ptr.cast::<Box>()) {
        return false;
    }
    let addr = ptr as usize;
    if box_ptr_cache_hit(&BOOL_BOX_PTR_CACHE, addr) {
        return true;
    }
    let present = BOOL_BOX_REGISTRY.with(|r| r.borrow().contains(&addr));
    if present {
        box_ptr_cache_record(&BOOL_BOX_PTR_CACHE, addr);
    }
    present
}

#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_BOX_ALLOC_BITS: extern "C" fn(i64) -> *mut Box = js_box_alloc_bits;

#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_BOX_RELEASE: extern "C" fn(*mut Box) = js_box_release;

#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_I32_BOX_RELEASE: extern "C" fn(*mut I32Box) = js_i32_box_release;

#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_BOOL_BOX_RELEASE: extern "C" fn(*mut BoolBox) = js_bool_box_release;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_BOX_GET_BITS: extern "C" fn(*mut Box) -> i64 = js_box_get_bits;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_BOX_SET_BITS: extern "C" fn(*mut Box, i64) = js_box_set_bits;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_BOX_ALLOC: extern "C" fn(f64) -> *mut Box = js_box_alloc;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_BOX_GET: extern "C" fn(*mut Box) -> f64 = js_box_get;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_BOX_SET: extern "C" fn(*mut Box, f64) = js_box_set;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_I32_BOX_ALLOC: extern "C" fn(i32) -> *mut I32Box = js_i32_box_alloc;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_I32_BOX_GET: extern "C" fn(*mut I32Box) -> i32 = js_i32_box_get;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_I32_BOX_SET: extern "C" fn(*mut I32Box, i32) = js_i32_box_set;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_BOOL_BOX_ALLOC: extern "C" fn(i32) -> *mut BoolBox = js_bool_box_alloc;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_BOOL_BOX_GET: extern "C" fn(*mut BoolBox) -> i32 = js_bool_box_get;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_BOOL_BOX_SET: extern "C" fn(*mut BoolBox, i32) = js_bool_box_set;

#[cfg(test)]
pub(crate) fn test_clear_box_registry() {
    crate::closure::test_clear_closure_box_capture_indexes();
    BOX_REGISTRY.with(|r| r.borrow_mut().clear());
    I32_BOX_REGISTRY.with(|r| r.borrow_mut().clear());
    BOOL_BOX_REGISTRY.with(|r| r.borrow_mut().clear());
    BOX_FREE_HEAD.with(|h| h.set(0));
    I32_BOX_FREE_HEAD.with(|h| h.set(0));
    BOOL_BOX_FREE_HEAD.with(|h| h.set(0));
    BOX_RELEASE_QUARANTINE.with(|q| q.borrow_mut().clear());
    I32_BOX_RELEASE_QUARANTINE.with(|q| q.borrow_mut().clear());
    BOOL_BOX_RELEASE_QUARANTINE.with(|q| q.borrow_mut().clear());
    ASYNC_RELEASED_CELLS.with(|cells| cells.borrow_mut().clear());
    ASYNC_PENDING_RELEASES.with(|pending| pending.borrow_mut().clear());
    // Registry membership is not monotonic any more (#8208: `js_*box_release`
    // de-registers a completed activation's cells), so the positive cache is
    // kept coherent by an eviction on every un-registration rather than by
    // never un-registering. This wholesale clear is the bulk case — it exists
    // only for tests — and it must drop the caches for the same reason a single
    // release evicts one slot: otherwise a later test would see a stale "yes"
    // for an address this call just un-registered.
    for cache in [&BOX_PTR_CACHE, &I32_BOX_PTR_CACHE, &BOOL_BOX_PTR_CACHE] {
        cache.with(|slots| {
            for slot in slots {
                slot.set(0);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// perry#4898: a structurally-plausible pointer that `js_box_alloc`
    /// never minted (here, a `&'static` read-only constant that is ≥0x1000,
    /// untagged, and 8-byte aligned — exactly the shape of the leaked
    /// `..._guard` string) must NOT be dereferenced by `js_box_set`. Before
    /// the registry check this stored into read-only memory → SIGBUS.
    #[test]
    fn box_set_skips_unregistered_plausible_pointer() {
        test_clear_box_registry();
        // 8-byte aligned static — passes every structural check, is not a box.
        static RODATA: [u64; 2] = [0xDEAD_BEEF, 0xFEED_FACE];
        let fake = (&RODATA[0] as *const u64) as *mut Box;
        assert!(is_plausible_box_ptr(fake), "test needs a plausible ptr");
        assert!(!is_registered_box_ptr(fake), "fake must not be registered");
        // Must be a silent no-op, not a write/crash.
        js_box_set(fake, 1.0);
        js_box_set_bits(
            fake,
            crate::value::JSValue::try_short_string(b"bad")
                .unwrap()
                .bits() as i64,
        );
        assert_eq!(RODATA[0], 0xDEAD_BEEF, "rodata must be untouched");
        // Reads from an unregistered pointer return `undefined` (perry#4926:
        // the read-before-initialization value of a boxed variable), never
        // deref. TAG_UNDEFINED is a NaN bit pattern, so this also preserves
        // the older "returns NaN" numeric behavior.
        assert_eq!(
            js_box_get_bits(fake) as u64,
            crate::value::TAG_UNDEFINED,
            "unregistered bits box read must yield undefined"
        );
        assert_eq!(
            js_box_get(fake).to_bits(),
            crate::value::TAG_UNDEFINED,
            "unregistered box read must yield undefined"
        );
    }

    /// A real `js_box_alloc` box still round-trips through set/get after the
    /// registry gate (no false negatives on genuine boxes).
    #[test]
    fn box_set_get_roundtrips_for_real_box() {
        test_clear_box_registry();
        let b = js_box_alloc(3.5);
        assert!(is_registered_box_ptr(b));
        assert_eq!(js_box_get(b), 3.5);
        js_box_set(b, 42.0);
        assert_eq!(js_box_get(b), 42.0);
    }

    /// The bits ABI is the canonical boxed-local storage path for dynamic
    /// JSValues. It must not turn Perry's NaN-boxed non-number values into a
    /// numeric NaN payload.
    #[test]
    fn box_bits_roundtrips_non_number_tags_exactly() {
        test_clear_box_registry();
        let cases = [
            crate::value::JSValue::int32(-17).bits(),
            crate::value::JSValue::try_short_string(b"ok")
                .unwrap()
                .bits(),
            crate::value::TAG_UNDEFINED,
        ];

        for bits in cases {
            let b = js_box_alloc_bits(bits as i64);
            assert!(is_registered_box_ptr(b));
            assert_eq!(js_box_get_bits(b) as u64, bits);
            assert_eq!(js_box_get(b).to_bits(), bits);

            let replacement = crate::value::JSValue::try_short_string(b"next")
                .unwrap()
                .bits();
            js_box_set_bits(b, replacement as i64);
            assert_eq!(js_box_get_bits(b) as u64, replacement);
            assert_eq!(js_box_get(b).to_bits(), replacement);
        }
    }

    #[test]
    fn primitive_control_boxes_round_trip_and_reject_foreign_pointers() {
        test_clear_box_registry();
        let i32_box = js_i32_box_alloc(7);
        assert!(is_registered_i32_box_ptr(i32_box));
        assert_eq!(js_i32_box_get(i32_box), 7);
        js_i32_box_set(i32_box, -3);
        assert_eq!(js_i32_box_get(i32_box), -3);

        let bool_box = js_bool_box_alloc(0);
        assert!(is_registered_bool_box_ptr(bool_box));
        assert_eq!(js_bool_box_get(bool_box), 0);
        js_bool_box_set(bool_box, 1);
        assert_eq!(js_bool_box_get(bool_box), 1);

        let ordinary_box = js_box_alloc(1.0);
        assert_eq!(js_i32_box_get(ordinary_box.cast::<I32Box>()), 0);
        js_i32_box_set(ordinary_box.cast::<I32Box>(), 99);
        assert_eq!(js_box_get(ordinary_box), 1.0);
    }

    /// The direct-mapped positive cache in front of `BOX_REGISTRY` must not
    /// widen what counts as a box. Sabotage shape: warm the cache with a real
    /// box, then probe a plausible-but-unregistered address that lands in the
    /// SAME cache slot. A cache that compared only the slot index (rather than
    /// the full address) would answer "registered" and `js_box_set` would then
    /// write through a pointer perry#4898 exists to reject.
    #[test]
    fn box_ptr_cache_rejects_a_colliding_unregistered_address() {
        test_clear_box_registry();
        let real = js_box_alloc_bits(crate::value::JSValue::int32(5).bits() as i64);
        assert!(
            is_registered_box_ptr(real),
            "warm the cache with a real box"
        );

        // Every 8-byte-aligned address whose (addr >> 3) is congruent mod the
        // slot count collides with `real`. Walk candidates until one is both
        // plausible and unregistered — `real + 8 * SLOTS * k` is guaranteed to
        // collide by construction.
        let real_addr = real as usize;
        let mut collided = 0usize;
        for k in 1..64usize {
            let candidate = real_addr + 8 * BOX_PTR_CACHE_SLOTS * k;
            let candidate_ptr = candidate as *mut Box;
            if !is_plausible_box_ptr(candidate_ptr) {
                continue;
            }
            if BOX_REGISTRY.with(|r| r.borrow().contains(&candidate)) {
                continue;
            }
            assert_eq!(
                box_ptr_cache_index(candidate),
                box_ptr_cache_index(real_addr),
                "candidate must map to the same cache slot"
            );
            assert!(
                !is_registered_box_ptr(candidate_ptr),
                "a colliding unregistered address must still be rejected"
            );
            collided += 1;
            if collided == 4 {
                break;
            }
        }
        assert!(
            collided > 0,
            "no colliding candidate found — test is vacuous"
        );

        // And the real box still reads back correctly after those misses.
        assert_eq!(
            js_box_get_bits(real) as u64,
            crate::value::JSValue::int32(5).bits()
        );
    }

    /// A box evicted from the cache by later allocations is still recognised —
    /// the cache is an accelerator, never the source of truth.
    #[test]
    fn box_ptr_cache_eviction_does_not_lose_a_real_box() {
        test_clear_box_registry();
        let first = js_box_alloc(1.0);
        assert!(is_registered_box_ptr(first));

        // Allocate well past the cache size so `first` is certainly evicted.
        let mut others = Vec::new();
        for i in 0..(BOX_PTR_CACHE_SLOTS * 8) {
            let b = js_box_alloc(i as f64);
            assert!(is_registered_box_ptr(b));
            others.push(b);
        }

        assert!(
            is_registered_box_ptr(first),
            "eviction must fall through to the authoritative registry"
        );
        assert_eq!(js_box_get(first), 1.0);
        js_box_set(first, 9.0);
        assert_eq!(js_box_get(first), 9.0);
    }

    /// The three registries have independent caches: an ordinary box address
    /// must never be accepted as an i32/bool box just because it is cached in
    /// the ordinary registry's table.
    #[test]
    fn box_ptr_caches_do_not_cross_kinds() {
        test_clear_box_registry();
        let ordinary = js_box_alloc(1.0);
        assert!(is_registered_box_ptr(ordinary));
        assert!(!is_registered_i32_box_ptr(ordinary.cast::<I32Box>()));
        assert!(!is_registered_bool_box_ptr(ordinary.cast::<BoolBox>()));

        let i32_box = js_i32_box_alloc(3);
        assert!(is_registered_i32_box_ptr(i32_box));
        assert!(!is_registered_box_ptr(i32_box.cast::<Box>()));
        assert!(!is_registered_bool_box_ptr(i32_box.cast::<BoolBox>()));
    }

    /// #6520: the thread-boundary serializer unwraps a capture slot that holds
    /// a box pointer to the value inside. `box_slot_contents_bits` returns the
    /// contained JSValue bits for a real box and `None` for anything else — a
    /// plain NaN-boxed value (high tag bits set → not a plausible box address),
    /// a plausible-but-unregistered pointer, and a null slot.
    #[test]
    fn box_slot_contents_unwraps_only_registered_boxes() {
        test_clear_box_registry();

        // A real box: returns the bits it holds, not the pointer.
        let inner = crate::value::JSValue::int32(1234).bits();
        let b = js_box_alloc_bits(inner as i64);
        let slot_bits = b as usize as u64; // codegen stores the raw box ptr here
        assert_eq!(box_slot_contents_bits(slot_bits), Some(inner));

        // A NaN-boxed non-box value (its own tag bits are set) is not a box.
        assert_eq!(box_slot_contents_bits(inner), None);
        assert_eq!(box_slot_contents_bits(crate::value::TAG_UNDEFINED), None);

        // A plausible pointer that was never minted as a box.
        static RODATA: [u64; 2] = [0xDEAD_BEEF, 0xFEED_FACE];
        let fake = (&RODATA[0] as *const u64) as usize as u64;
        assert!(is_plausible_box_ptr(fake as usize as *mut Box));
        assert_eq!(box_slot_contents_bits(fake), None);

        // Null / near-null slots.
        assert_eq!(box_slot_contents_bits(0), None);
    }
}

#[cfg(test)]
mod release_tests {
    use super::*;

    fn install_test_activation(activation: *mut AsyncBoxActivation) -> crate::promise::InlineTrap {
        crate::promise::INLINE_TRAP.with(|trap| {
            trap.replace(crate::promise::InlineTrap {
                trap_next: std::ptr::null_mut(),
                current_step: 0,
                box_activation: activation,
            })
        })
    }

    /// `BOX_ALLOC_COUNT` / `BOX_POOL_REUSE_COUNT` / `BOX_RELEASE_COUNT` are
    /// process-global atomics, while the registries, quarantines and free
    /// lists they describe are THREAD-LOCAL. Any test that asserts on a
    /// counter *delta* is therefore not isolated by `test_clear_box_registry`
    /// alone — a sibling test allocating on another harness thread lands in
    /// the same atomics and moves the delta under it. Observed exactly that:
    /// these tests pass under `--test-threads=1` and fail in parallel.
    ///
    /// Serialise the counter-asserting tests against each other. Tests that
    /// only assert on addresses and registry membership are thread-local and
    /// need no lock.
    fn counter_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        // A panicking test poisons the lock; the data is `()`, so recovering
        // is right — otherwise one failure cascades into spurious ones.
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A released cell must be INERT: de-registered (reads `undefined`,
    /// writes dropped), evicted from the positive cache, and parked exactly
    /// once no matter how many times the terminal arm re-runs (#7933
    /// follow-up; the stray-duplicate-resume path re-runs the release list).
    #[test]
    fn released_cell_is_inert_and_release_is_idempotent() {
        super::test_clear_box_registry();
        let ptr = js_box_alloc_bits(crate::value::TAG_TRUE as i64);
        assert!(is_registered_box_ptr(ptr));
        js_box_release(ptr);
        assert!(
            !is_registered_box_ptr(ptr),
            "released cell must be de-registered (and cache-evicted)"
        );
        assert_eq!(
            js_box_get_bits(ptr) as u64,
            crate::value::TAG_UNDEFINED,
            "released cell must read undefined"
        );
        js_box_set_bits(ptr, crate::value::TAG_TRUE as i64);
        assert_eq!(
            unsafe { (*ptr).value },
            crate::value::TAG_UNDEFINED,
            "write to a released cell must be dropped"
        );
        // Idempotence: a second release must not double-park the address —
        // a double-park would hand the same cell to two future activations.
        js_box_release(ptr);
        js_box_release(ptr);
        let parked = BOX_RELEASE_QUARANTINE
            .with(|q| q.borrow().iter().filter(|&&a| a == ptr as usize).count());
        assert_eq!(parked, 1, "double release must park exactly once");
    }

    /// Fallback reuse contract: an untracked released cell becomes allocatable
    /// only AFTER the outermost-pump quarantine flush, and the reused cell is
    /// re-registered with the fresh initial value.
    #[test]
    fn released_cell_is_reused_only_after_flush() {
        super::test_clear_box_registry();
        let first = js_box_alloc_bits(1.0f64.to_bits() as i64);
        js_box_release(first);
        // Not flushed yet: allocation must NOT reuse the parked cell.
        let second = js_box_alloc_bits(2.0f64.to_bits() as i64);
        assert_ne!(
            first as usize, second as usize,
            "quarantined cell must not be reused before the flush boundary"
        );
        flush_released_boxes();
        let third = js_box_alloc_bits(3.0f64.to_bits() as i64);
        assert_eq!(
            first as usize, third as usize,
            "flushed cell must be reused by the next allocation"
        );
        assert!(is_registered_box_ptr(third), "reused cell re-registers");
        assert_eq!(js_box_get_bits(third), 3.0f64.to_bits() as i64);
    }

    /// The #8208 floor-closing contract: terminal release alone does not make
    /// a cell reusable while another queued resume still owns the activation.
    /// The last queued/running-reference decrement publishes it immediately,
    /// without waiting for the whole thread's task queue to drain.
    #[test]
    fn activation_cells_publish_at_its_reachability_zero() {
        test_clear_box_registry();
        let activation = new_async_box_activation(); // lifecycle owner
        retain_async_box_activation(activation); // currently running step
        retain_async_box_activation(activation); // duplicate queued step
        let previous = install_test_activation(activation);

        let released = js_box_alloc_bits(1.0f64.to_bits() as i64);
        js_box_release(released); // also drops the lifecycle owner

        let before_zero = js_box_alloc_bits(2.0f64.to_bits() as i64);
        assert_ne!(
            released, before_zero,
            "a queued resume still reaches the frame"
        );
        release_async_box_activation(activation); // running step exits
        let still_reachable = js_box_alloc_bits(3.0f64.to_bits() as i64);
        assert_ne!(
            released, still_reachable,
            "duplicate task still owns the frame"
        );

        release_async_box_activation(activation); // duplicate dispatch exits
        let after_zero = js_box_alloc_bits(4.0f64.to_bits() as i64);
        assert_eq!(
            released, after_zero,
            "the final decrement must publish immediately"
        );
        crate::promise::INLINE_TRAP.with(|trap| trap.set(previous));
    }

    /// #8213: a closure may outlive the async function that created it. Its
    /// captured box remains readable after terminal release, and a child
    /// closure created later increments that same cell's capture count. Only
    /// death of the last such closure publishes the cell.
    #[test]
    fn escaped_closures_defer_activation_cell_publication_until_gc_death() {
        test_clear_box_registry();
        let activation = new_async_box_activation();
        retain_async_box_activation(activation); // running step
        let previous = install_test_activation(activation);

        let cell = js_box_alloc_bits(crate::value::JSValue::int32(41).bits() as i64);
        let uncaptured = js_box_alloc_bits(crate::value::JSValue::int32(40).bits() as i64);
        let outer = crate::closure::js_closure_alloc(std::ptr::null(), 1);
        crate::closure::js_closure_set_box_capture_ptr(outer, 0, cell as i64);
        crate::closure::js_closure_set_box_capture_ptr(outer, 0, cell as i64);
        assert_eq!(
            crate::closure::box_capture_count(cell as usize),
            1,
            "a singleton cache hit may redeclare the same slot idempotently"
        );

        js_box_release(cell); // terminal lifecycle owner drops
        js_box_release(uncaptured);
        release_async_box_activation(activation); // running step exits
        assert!(is_registered_box_ptr(cell));
        assert!(
            !is_registered_box_ptr(uncaptured),
            "one captured cell must not retain the rest of the async frame"
        );
        assert_eq!(
            js_box_get_bits(cell) as u64,
            crate::value::JSValue::int32(41).bits(),
            "an escaped closure must still observe its captured value"
        );

        // Model invoking `outer` after the async activation returned: no
        // ambient async token exists, but the registered pending cell still
        // lets a nested closure acquire its own capture count.
        crate::promise::INLINE_TRAP.with(|trap| trap.set(crate::promise::InlineTrap::empty()));
        let child = crate::closure::js_closure_alloc(std::ptr::null(), 1);
        crate::closure::js_closure_set_box_capture_ptr(child, 0, cell as i64);
        crate::closure::prune_dead_closure_box_capture_owners(&|owner| owner == outer as usize);
        assert!(
            is_registered_box_ptr(cell),
            "the child closure remains live"
        );

        crate::closure::prune_dead_closure_box_capture_owners(&|owner| owner == child as usize);
        assert!(!is_registered_box_ptr(cell));
        let reused = js_box_alloc_bits(crate::value::JSValue::int32(42).bits() as i64);
        assert_eq!(cell, reused, "the last closure death publishes the cell");
        crate::promise::INLINE_TRAP.with(|trap| trap.set(previous));
    }

    /// The two halves of the full-trace ownership rule must move together:
    /// the drained box stops being a global root, while a closure already
    /// proven live exposes that box's JSValue payload as its external child.
    /// Otherwise a self-cycle is immortal (first half missing) or an escaped
    /// closure observes a collected payload (second half missing).
    #[test]
    fn full_trace_treats_drained_closure_boxes_as_ephemeron_edges() {
        test_clear_box_registry();
        let activation = new_async_box_activation();
        retain_async_box_activation(activation);
        let previous = install_test_activation(activation);
        let cell = js_box_alloc_bits(crate::value::TAG_UNDEFINED as i64);
        let closure = crate::closure::js_closure_alloc(std::ptr::null(), 1);
        crate::closure::js_closure_set_box_capture_ptr(closure, 0, cell as i64);
        let closure_bits = crate::value::js_nanbox_pointer(closure as i64).to_bits();
        js_box_set_bits(cell, closure_bits as i64);
        js_box_release(cell);
        release_async_box_activation(activation);
        assert!(is_registered_box_ptr(cell));

        crate::gc::begin_full_trace();
        let mut rooted = Vec::new();
        scan_box_roots(&mut |value| rooted.push(value.to_bits()));
        assert!(
            !rooted.contains(&closure_bits),
            "the drained box must not root its own closure during a full trace"
        );

        let header = unsafe {
            (closure as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader
        };
        let saved_flags = unsafe { (*header).gc_flags };
        unsafe { (*header).gc_flags |= crate::gc::GC_FLAG_MARKED };
        let slots = crate::gc::test_gc_rewrite_slot_addresses(closure as usize)
            .expect("closure rewrite descriptor");
        assert!(
            slots.contains(&(cell as usize)),
            "a marked closure must trace the drained box payload"
        );
        unsafe { (*header).gc_flags = saved_flags };
        crate::gc::finish_full_trace();

        crate::closure::prune_dead_closure_box_capture_owners(&|owner| owner == closure as usize);
        assert!(!is_registered_box_ptr(cell));
        crate::promise::INLINE_TRAP.with(|trap| trap.set(previous));
    }

    /// Pointer-shaped generic captures are common in runtime-created
    /// closures. Even if their bits happen to equal a live box address, only
    /// the compiler-declared setter may create a box lifetime edge.
    #[test]
    fn generic_pointer_capture_does_not_retain_an_async_box() {
        test_clear_box_registry();
        let activation = new_async_box_activation();
        retain_async_box_activation(activation);
        let previous = install_test_activation(activation);
        let cell = js_box_alloc_bits(crate::value::JSValue::int32(5).bits() as i64);
        let closure = crate::closure::js_closure_alloc(std::ptr::null(), 1);

        crate::closure::js_closure_set_capture_ptr(closure, 0, cell as i64);
        assert_eq!(crate::closure::box_capture_count(cell as usize), 0);
        js_box_release(cell);
        release_async_box_activation(activation);
        assert!(!is_registered_box_ptr(cell));

        crate::closure::prune_dead_closure_box_capture_owners(&|owner| owner == closure as usize);
        crate::promise::INLINE_TRAP.with(|trap| trap.set(previous));
    }

    /// A closure created during an async step can also capture a mutable box
    /// owned by an outer scope. The raw capture count must not make that box a
    /// terminal-release candidate for the ambient activation.
    #[test]
    fn outer_scope_box_capture_is_not_bound_to_the_ambient_activation() {
        test_clear_box_registry();
        let outer_cell = js_box_alloc_bits(crate::value::JSValue::int32(7).bits() as i64);
        let activation = new_async_box_activation();
        retain_async_box_activation(activation);
        let previous = install_test_activation(activation);

        let closure = crate::closure::js_closure_alloc(std::ptr::null(), 1);
        crate::closure::js_closure_set_box_capture_ptr(closure, 0, outer_cell as i64);
        assert_eq!(crate::closure::box_capture_count(outer_cell as usize), 1);

        let frame_cell = js_box_alloc_bits(crate::value::JSValue::int32(8).bits() as i64);
        js_box_release(frame_cell);
        release_async_box_activation(activation);
        assert!(!is_registered_box_ptr(frame_cell));
        assert!(is_registered_box_ptr(outer_cell));
        assert_eq!(
            js_box_get_bits(outer_cell) as u64,
            crate::value::JSValue::int32(7).bits()
        );
        crate::closure::prune_dead_closure_box_capture_owners(&|owner| owner == closure as usize);
        crate::promise::INLINE_TRAP.with(|trap| trap.set(previous));
    }

    #[test]
    fn closure_move_rekeys_the_capture_count_before_death_pruning() {
        test_clear_box_registry();
        let activation = new_async_box_activation();
        retain_async_box_activation(activation);
        let previous = install_test_activation(activation);
        let cell = js_box_alloc_bits(crate::value::JSValue::int32(9).bits() as i64);
        let closure = crate::closure::js_closure_alloc(std::ptr::null(), 1);
        crate::closure::js_closure_set_box_capture_ptr(closure, 0, cell as i64);
        js_box_release(cell);
        release_async_box_activation(activation);

        let moved = crate::closure::js_closure_alloc(std::ptr::null(), 1);
        crate::closure::closure_box_captures_owner_moved(closure as usize, moved as usize);
        crate::closure::prune_dead_closure_box_capture_owners(&|owner| owner == closure as usize);
        assert!(
            is_registered_box_ptr(cell),
            "the old address no longer owns the retain"
        );
        crate::closure::prune_dead_closure_box_capture_owners(&|owner| owner == moved as usize);
        assert!(!is_registered_box_ptr(cell));
        crate::promise::INLINE_TRAP.with(|trap| trap.set(previous));
    }

    #[test]
    fn runtime_closure_clone_copies_exact_box_capture_edges() {
        test_clear_box_registry();
        let activation = new_async_box_activation();
        retain_async_box_activation(activation);
        let previous = install_test_activation(activation);
        let cell = js_box_alloc_bits(crate::value::JSValue::int32(10).bits() as i64);
        let source = crate::closure::js_closure_alloc(std::ptr::null(), 1);
        crate::closure::js_closure_set_box_capture_ptr(source, 0, cell as i64);
        let cloned = crate::closure::js_closure_alloc(std::ptr::null(), 1);
        crate::closure::clone_closure_box_captures(source, cloned);
        assert_eq!(crate::closure::box_capture_count(cell as usize), 2);

        js_box_release(cell);
        release_async_box_activation(activation);
        crate::closure::prune_dead_closure_box_capture_owners(&|owner| owner == source as usize);
        assert!(
            is_registered_box_ptr(cell),
            "the clone retains its exact edge"
        );
        crate::closure::prune_dead_closure_box_capture_owners(&|owner| owner == cloned as usize);
        assert!(!is_registered_box_ptr(cell));
        crate::promise::INLINE_TRAP.with(|trap| trap.set(previous));
    }

    /// Pending-await thunks carry a raw malloc-token pointer so the moving GC
    /// cannot invalidate it. Reusing that token must not make an old thunk
    /// name a new activation; the captured generation is the discriminator.
    #[test]
    fn recycled_activation_token_rejects_a_stale_generation() {
        test_clear_box_registry();
        let first = new_async_box_activation();
        let first_id = async_box_activation_id(first);
        assert_eq!(find_async_box_activation(first, first_id), first);
        finish_async_box_activation(first);
        assert!(find_async_box_activation(first, first_id).is_null());

        let second = new_async_box_activation();
        let second_id = async_box_activation_id(second);
        assert_eq!(second, first, "the test must exercise token recycling");
        assert_ne!(second_id, first_id);
        assert!(find_async_box_activation(second, first_id).is_null());
        assert_eq!(find_async_box_activation(second, second_id), second);
        finish_async_box_activation(second);
    }

    /// Reachability is per activation, not a renamed global queue-empty gate:
    /// B's completed frame is reusable while A still has a stale queued task.
    #[test]
    fn one_activation_does_not_quarantine_an_unrelated_completed_frame() {
        test_clear_box_registry();

        let activation_a = new_async_box_activation();
        retain_async_box_activation(activation_a); // running
        retain_async_box_activation(activation_a); // delayed duplicate
        let previous = install_test_activation(activation_a);
        let a = js_box_alloc_bits(10.0f64.to_bits() as i64);
        js_box_release(a);
        release_async_box_activation(activation_a); // leave duplicate alive

        let activation_b = new_async_box_activation();
        retain_async_box_activation(activation_b); // running
        install_test_activation(activation_b);
        let b = js_box_alloc_bits(20.0f64.to_bits() as i64);
        js_box_release(b);
        release_async_box_activation(activation_b); // B reaches zero

        let reused_b = js_box_alloc_bits(30.0f64.to_bits() as i64);
        assert_eq!(
            b, reused_b,
            "B must publish independently of A's queued task"
        );
        assert_ne!(a, reused_b, "A must remain parked");

        release_async_box_activation(activation_a);
        let reused_a = js_box_alloc_bits(40.0f64.to_bits() as i64);
        assert_eq!(a, reused_a, "A publishes when its own duplicate exits");
        crate::promise::INLINE_TRAP.with(|trap| trap.set(previous));
    }

    /// The pump's setjmp recovery must release the inner task reference that
    /// `longjmp` skipped, while leaving a re-entrant caller's activation below
    /// the saved depth untouched.
    #[test]
    fn exception_unwind_releases_only_this_pumps_activation_refs() {
        test_clear_box_registry();
        let base_depth = crate::promise::async_box_execution_ref_depth();

        let outer = new_async_box_activation();
        retain_async_box_activation(outer); // running owner
        crate::promise::push_async_box_execution_ref(outer);
        let previous = install_test_activation(outer);
        let outer_cell = js_box_alloc_bits(1.0f64.to_bits() as i64);
        js_box_release(outer_cell); // drop outer lifecycle owner

        let nested_depth = crate::promise::async_box_execution_ref_depth();
        let inner = new_async_box_activation();
        retain_async_box_activation(inner); // running owner skipped by longjmp
        crate::promise::push_async_box_execution_ref(inner);
        install_test_activation(inner);
        let inner_cell = js_box_alloc_bits(2.0f64.to_bits() as i64);
        js_box_release(inner_cell); // drop inner lifecycle owner

        crate::promise::unwind_async_box_execution_refs(nested_depth);
        let reused_inner = js_box_alloc_bits(3.0f64.to_bits() as i64);
        assert_eq!(
            inner_cell, reused_inner,
            "inner unwind must release its owner"
        );
        assert_ne!(
            outer_cell, reused_inner,
            "outer owner is below nested depth"
        );

        crate::promise::pop_async_box_execution_ref(outer);
        release_async_box_activation(outer);
        assert_eq!(
            crate::promise::async_box_execution_ref_depth(),
            base_depth,
            "test must restore the execution-ref stack"
        );
        let reused_outer = js_box_alloc_bits(4.0f64.to_bits() as i64);
        assert_eq!(outer_cell, reused_outer, "outer publishes at its own tail");
        crate::promise::INLINE_TRAP.with(|trap| trap.set(previous));
    }

    /// Generated async-step code reads the compiler-private control cells
    /// with RAW loads (`load_async_i32_control_cell` /
    /// `load_async_i1_control_cell`), never through the registry-checked
    /// getters — so the PARKED VALUES are load-bearing: a stray duplicate
    /// resume must observe `__gen_done == true` (the terminal short-circuit)
    /// and, were it ever to read state, `-1` (no dispatch case matches).
    #[test]
    fn typed_control_cells_park_terminal_values() {
        super::test_clear_box_registry();
        let state = js_i32_box_alloc(7);
        let done = js_bool_box_alloc(0);
        js_i32_box_release(state);
        js_bool_box_release(done);
        assert_eq!(
            unsafe { (*state).value },
            -1,
            "parked i32 control cell must raw-read as -1 (no state)"
        );
        assert!(
            unsafe { (*done).value },
            "parked i1 control cell must raw-read as true (done)"
        );
        // And the checked getters treat them as not-a-box.
        assert_eq!(js_i32_box_get(state), 0);
        assert_eq!(js_bool_box_get(done), 0);
    }

    /// The intrusive free list must round-trip a WHOLE cohort, not just one
    /// cell. Each free cell's own 8 bytes hold the link to the next, so a
    /// mis-written link would either lose most of the pool (silently
    /// reverting to `std::alloc` and re-growing the residue) or splice a cell
    /// in twice and hand one address to two live activations.
    ///
    /// Asserts all three: every cell comes back, each exactly once, and each
    /// carries its own fresh value rather than a leftover link.
    #[test]
    fn the_intrusive_free_list_round_trips_a_whole_cohort() {
        let _guard = counter_guard();
        super::test_clear_box_registry();
        const N: usize = 512;
        let first: Vec<*mut Box> = (0..N)
            .map(|i| js_box_alloc_bits((i as f64).to_bits() as i64))
            .collect();
        let minted: std::collections::HashSet<usize> = first.iter().map(|p| *p as usize).collect();
        assert_eq!(minted.len(), N, "the fixture must mint N distinct cells");

        for p in &first {
            js_box_release(*p);
        }
        flush_released_boxes();

        let (a0, r0, _) = box_release_stats();
        let second: Vec<*mut Box> = (0..N)
            .map(|i| js_box_alloc_bits((1000.0 + i as f64).to_bits() as i64))
            .collect();
        let (a1, r1, _) = box_release_stats();
        assert_eq!(a1 - a0, N as u64, "second cohort allocates N cells");
        assert_eq!(
            r1 - r0,
            N as u64,
            "ALL N must come from the free list; {} fell through to std::alloc",
            N as u64 - (r1 - r0)
        );

        let reused: std::collections::HashSet<usize> = second.iter().map(|p| *p as usize).collect();
        assert_eq!(reused.len(), N, "an address was handed out twice");
        assert_eq!(
            reused, minted,
            "reused cells must be exactly the minted set"
        );

        for (i, p) in second.iter().enumerate() {
            assert_eq!(
                js_box_get_bits(*p),
                (1000.0 + i as f64).to_bits() as i64,
                "cell {i} kept a stale free-list link instead of its value"
            );
        }
        // Drained: the next allocation has to mint.
        let before = box_release_stats().1;
        let _fresh = js_box_alloc_bits(0);
        assert_eq!(
            box_release_stats().1,
            before,
            "the list was drained, so this must be a fresh std::alloc"
        );
    }

    /// perry#4898 discipline extends to release: a structurally-plausible
    /// pointer that was never minted as a box must be a TOTAL no-op — no
    /// deref, no park.
    #[test]
    fn foreign_pointer_release_is_a_total_noop() {
        super::test_clear_box_registry();
        static RODATA: [u64; 2] = [0xDEAD_BEEF, 0xFEED_FACE];
        let fake = (&RODATA[0] as *const u64) as *mut Box;
        js_box_release(fake);
        assert_eq!(RODATA[0], 0xDEAD_BEEF, "rodata must be untouched");
        let parked = BOX_RELEASE_QUARANTINE.with(|q| q.borrow().len());
        assert_eq!(parked, 0, "foreign pointer must not be parked");
    }

    /// THE #7933-follow-up regression gate, as a counter assertion (the leak
    /// is behaviorally invisible — a test that merely runs to completion
    /// cannot fail on it). Simulate N async-activation lifecycles (alloc a
    /// frame of cells, release it at terminal, hit the drain boundary every
    /// "turn"): the malloc-side residue — cells that cost a real
    /// `std::alloc` allocation, `allocs - pool_reuses` — must stay bounded
    /// by one turn's working set instead of growing linearly with N. Before
    /// the release/reuse machinery existed, residue == every cell ever
    /// allocated (~500 B/activation of cells + registry, 119 MB on
    /// asyncpipe_big).
    #[test]
    fn completed_activation_residue_is_bounded_not_linear() {
        let _guard = counter_guard();
        super::test_clear_box_registry();
        const TURNS: usize = 100;
        const ACTIVATIONS_PER_TURN: usize = 20;
        // handle()-shaped frame: 3 JSValue cells + 1 i32 + 2 bool controls.
        const CELLS_PER_ACTIVATION: usize = 6;
        let (a0, r0, _) = box_release_stats();
        let mut distinct = std::collections::HashSet::new();
        for _ in 0..TURNS {
            for _ in 0..ACTIVATIONS_PER_TURN {
                let b1 = js_box_alloc_bits(crate::value::TAG_UNDEFINED as i64);
                let b2 = js_box_alloc_bits(crate::value::TAG_UNDEFINED as i64);
                let b3 = js_box_alloc_bits(crate::value::TAG_UNDEFINED as i64);
                let state = js_i32_box_alloc(0);
                let done = js_bool_box_alloc(0);
                let exec = js_bool_box_alloc(0);
                for b in [b1, b2, b3] {
                    distinct.insert(b as usize);
                }
                distinct.insert(state as usize);
                distinct.insert(done as usize);
                distinct.insert(exec as usize);
                // Terminal state: release the whole frame.
                for b in [b1, b2, b3] {
                    js_box_release(b);
                }
                js_i32_box_release(state);
                js_bool_box_release(done);
                js_bool_box_release(exec);
            }
            // Outermost microtask-pump boundary, task queue empty.
            flush_released_boxes();
        }
        let (a1, r1, _) = box_release_stats();
        // The counters are process-global; sibling tests on other threads
        // also allocate boxes, so assert lower bounds and give the residue
        // bound slack instead of demanding exact equality.
        let total_allocs = (a1 - a0) as usize;
        let residue = total_allocs.saturating_sub((r1 - r0) as usize);
        let own_allocs = TURNS * ACTIVATIONS_PER_TURN * CELLS_PER_ACTIVATION;
        assert!(
            total_allocs >= own_allocs,
            "every lifecycle allocates its frame ({total_allocs} < {own_allocs})"
        );
        // One turn's working set (the first turn mints real cells; every
        // later turn reuses them), plus generous slack for whatever the
        // parallel sibling tests allocate (they use a handful of cells
        // each). The pre-fix residue is TURNS * the per-turn bound, two
        // orders of magnitude past this.
        let bound = 4 * ACTIVATIONS_PER_TURN * CELLS_PER_ACTIVATION;
        assert!(
            residue <= bound,
            "malloc residue must be bounded by one turn's working set: \
             residue={residue} bound={bound} (linear would be {total_allocs})"
        );
        assert!(
            distinct.len() <= bound,
            "distinct cell addresses must be bounded (got {})",
            distinct.len()
        );
        // The registries hold only the (small) final turn's live set — the
        // linear-growth signature is gone from the scan population too.
        let reg_total = BOX_REGISTRY.with(|r| r.borrow().len())
            + I32_BOX_REGISTRY.with(|r| r.borrow().len())
            + BOOL_BOX_REGISTRY.with(|r| r.borrow().len());
        assert!(
            reg_total <= bound,
            "registry population must not scale with completed activations \
             (got {reg_total})"
        );
    }
}
